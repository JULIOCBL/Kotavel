#include <kotavel/http/request.hpp>
#include <kotavel/http/session.hpp>

#include <rapidjson/document.h>

#include <algorithm>
#include <cctype>
#include <stdexcept>
#include <string>
#include <utility>

namespace kotavel::http {
namespace {
thread_local const Request* active_request = nullptr;

std::string lower(std::string value) {
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    return value;
}

std::string trim(std::string value) {
    const auto first = std::find_if_not(value.begin(), value.end(), [](unsigned char ch) {
        return std::isspace(ch) != 0;
    });
    const auto last = std::find_if_not(value.rbegin(), value.rend(), [](unsigned char ch) {
        return std::isspace(ch) != 0;
    }).base();
    if (first >= last) return {};
    return std::string(first, last);
}

int hex_value(char ch) {
    if (ch >= '0' && ch <= '9') return ch - '0';
    if (ch >= 'a' && ch <= 'f') return ch - 'a' + 10;
    if (ch >= 'A' && ch <= 'F') return ch - 'A' + 10;
    return -1;
}

std::string decode_component(std::string_view value) {
    std::string decoded;
    decoded.reserve(value.size());
    for (std::size_t index = 0; index < value.size(); ++index) {
        if (value[index] == '+' ) {
            decoded.push_back(' ');
        } else if (value[index] == '%' && index + 2 < value.size()) {
            const int high = hex_value(value[index + 1]);
            const int low = hex_value(value[index + 2]);
            if (high >= 0 && low >= 0) {
                decoded.push_back(static_cast<char>((high << 4) | low));
                index += 2;
            } else {
                decoded.push_back(value[index]);
            }
        } else {
            decoded.push_back(value[index]);
        }
    }
    return decoded;
}

using Values = std::map<std::string, std::optional<std::string>, std::less<>>;
void store_value(Values& values, std::string key, std::optional<std::string> value);

std::string quoted_parameter(std::string_view text, std::string_view key) {
    const std::string needle = std::string(key) + "=\"";
    const auto start = text.find(needle);
    if (start == std::string_view::npos) return {};
    const auto value_start = start + needle.size();
    const auto end = text.find('"', value_start);
    return end == std::string_view::npos ? std::string{} :
        std::string(text.substr(value_start, end - value_start));
}

void parse_multipart(std::string_view body, std::string_view boundary, Values& fields,
                     std::map<std::string, UploadedFile, std::less<>>& files) {
    const std::string delimiter = "--" + std::string(boundary);
    std::size_t cursor = 0;
    while (true) {
        const auto start = body.find(delimiter, cursor);
        if (start == std::string_view::npos) break;
        cursor = start + delimiter.size();
        if (body.substr(cursor, 2) == "--") break;
        if (body.substr(cursor, 2) == "\r\n") cursor += 2;
        const auto header_end = body.find("\r\n\r\n", cursor);
        if (header_end == std::string_view::npos) break;
        const auto header_block = body.substr(cursor, header_end - cursor);
        const auto content_start = header_end + 4;
        const auto content_end = body.find("\r\n" + delimiter, content_start);
        if (content_end == std::string_view::npos) break;
        std::string disposition;
        std::string content_type;
        std::size_t line_start = 0;
        while (line_start < header_block.size()) {
            const auto line_end = header_block.find("\r\n", line_start);
            const auto line = header_block.substr(line_start, line_end == std::string_view::npos
                ? header_block.size() - line_start : line_end - line_start);
            const auto colon = line.find(':');
            if (colon != std::string_view::npos) {
                const auto name = lower(std::string(line.substr(0, colon)));
                auto value = line.substr(colon + 1);
                while (!value.empty() && std::isspace(static_cast<unsigned char>(value.front())))
                    value.remove_prefix(1);
                if (name == "content-disposition") disposition = std::string(value);
                else if (name == "content-type") content_type = lower(std::string(value));
            }
            if (line_end == std::string_view::npos) break;
            line_start = line_end + 2;
        }
        const auto name = quoted_parameter(disposition, "name");
        const auto filename = quoted_parameter(disposition, "filename");
        const auto content = body.substr(content_start, content_end - content_start);
        if (!name.empty() && !filename.empty()) {
            files.insert_or_assign(name, UploadedFile{filename, content_type, std::string(content)});
        } else if (!name.empty()) {
            store_value(fields, name, std::string(content));
        }
        cursor = content_end + 2;
    }
}

void store_value(Values& values, std::string key, std::optional<std::string> value) {
    if (value) {
        auto cleaned = trim(std::move(*value));
        if (cleaned.empty()) value.reset();
        else value = std::move(cleaned);
    }
    values.insert_or_assign(std::move(key), std::move(value));
}

void parse_url_encoded(std::string_view encoded, Values& values) {
    while (!encoded.empty()) {
        const auto end = encoded.find('&');
        const auto pair = encoded.substr(0, end);
        const auto equal = pair.find('=');
        const auto key = decode_component(pair.substr(0, equal));
        if (!key.empty()) {
            const auto value = equal == std::string_view::npos
                ? std::string{} : decode_component(pair.substr(equal + 1));
            store_value(values, key, value);
        }
        if (end == std::string_view::npos) break;
        encoded.remove_prefix(end + 1);
    }
}

std::string scalar_value(const rapidjson::Value& value) {
    if (value.IsString()) return {value.GetString(), value.GetStringLength()};
    if (value.IsBool()) return value.GetBool() ? "true" : "false";
    if (value.IsInt64()) return std::to_string(value.GetInt64());
    if (value.IsUint64()) return std::to_string(value.GetUint64());
    if (value.IsDouble()) return std::to_string(value.GetDouble());
    return {};
}

void flatten_json(const rapidjson::Value& value, const std::string& key, Values& values,
                 std::set<std::string, std::less<>>& arrays, std::set<std::string, std::less<>>& objects,
                 std::map<std::string, std::vector<std::string>, std::less<>>& array_values,
                 std::map<std::string, std::vector<std::string>, std::less<>>& object_keys) {
    if (value.IsObject()) {
        objects.insert(key);
        for (auto member = value.MemberBegin(); member != value.MemberEnd(); ++member) {
            const std::string member_name(member->name.GetString(), member->name.GetStringLength());
            object_keys[key].push_back(member_name);
            const std::string child_key = key.empty()
                ? member_name : key + "." + member_name;
            flatten_json(member->value, child_key, values, arrays, objects, array_values,
                         object_keys);
        }
        return;
    }
    if (value.IsArray()) {
        arrays.insert(key);
        for (rapidjson::SizeType index = 0; index < value.Size(); ++index) {
            const auto child_key = key.empty() ? std::to_string(index)
                                                : key + "." + std::to_string(index);
            if (value[index].IsObject() || value[index].IsArray()) {
                array_values[key].push_back("[complex]");
            } else if (value[index].IsNull()) {
                array_values[key].push_back("");
            } else {
                array_values[key].push_back(scalar_value(value[index]));
            }
            flatten_json(value[index], child_key, values, arrays, objects, array_values,
                         object_keys);
        }
        return;
    }
    if (value.IsNull()) store_value(values, key, std::nullopt);
    else store_value(values, key, scalar_value(value));
}

} // namespace

Request::Request(RequestData data)
    : method_(data.method), kind_(data.kind), path_(std::move(data.path)),
      body_(std::move(data.body)), raw_method_(std::move(data.raw_method)),
      query_string_(std::move(data.query_string)),
      route_parameters_(std::move(data.route_parameters)) {
    for (auto& [name, value] : data.headers)
        headers_.insert_or_assign(lower(std::move(name)), std::move(value));

    parse_url_encoded(query_string_, query_values_);
    const auto content_type_header = header("content-type").value_or(std::string_view{});
    const auto content_type = lower(std::string(content_type_header));
    if (content_type.starts_with("application/json")) {
        if (body_.empty()) return;
        rapidjson::Document document;
        document.Parse(body_.data(), body_.size());
        if (document.HasParseError()) {
            input_valid_ = false;
            return;
        }
        flatten_json(document, {}, body_values_, body_arrays_, body_objects_,
                     body_array_values_, body_object_keys_);
    } else if (content_type.starts_with("application/x-www-form-urlencoded")) {
        parse_url_encoded(body_, body_values_);
    } else if (content_type.starts_with("multipart/form-data")) {
        auto boundary = quoted_parameter(content_type_header, "boundary");
        if (boundary.empty()) {
            const auto marker = content_type.find("boundary=");
            if (marker != std::string::npos) {
                const auto start = marker + 9;
                const auto end = content_type.find(';', start);
                boundary = content_type.substr(start, end == std::string::npos
                    ? content_type.size() - start : end - start);
            }
        }
        const auto encoded_boundary = boundary.empty() ? std::string_view{} : std::string_view(boundary);
        if (!encoded_boundary.empty()) parse_multipart(body_, encoded_boundary, body_values_, files_);
    }
}

Request::Request(Method method, std::string_view path, std::string_view body,
                 std::string_view raw_method, RouteKind kind)
    : Request(RequestData{method, kind, std::string(path), std::string(body),
                          std::string(raw_method), {}, {}, {}}) {}

std::optional<std::string_view> Request::input(std::string_view key) const {
    const auto body_value = body_values_.find(key);
    if (body_value != body_values_.end()) {
        if (!body_value->second) return std::nullopt;
        return *body_value->second;
    }
    if (const auto* uploaded = file(key)) return uploaded->name;
    return query(key);
}

std::optional<std::string_view> Request::query(std::string_view key) const {
    const auto value = query_values_.find(key);
    if (value == query_values_.end() || !value->second) return std::nullopt;
    return *value->second;
}

std::optional<std::string_view> Request::route(std::string_view key) const {
    const auto value = route_parameters_.find(std::string(key));
    if (value == route_parameters_.end()) return std::nullopt;
    return value->second;
}

std::optional<std::string_view> Request::header(std::string_view key) const {
    // Most adapter/internal lookups are already lowercase. Only normalize when needed.
    const bool uppercase = std::any_of(key.begin(), key.end(), [](unsigned char ch) {
        return ch >= 'A' && ch <= 'Z';
    });
    const auto value = uppercase ? headers_.find(lower(std::string(key))) : headers_.find(key);
    if (value == headers_.end()) return std::nullopt;
    return value->second;
}

const UploadedFile* Request::file(std::string_view key) const {
    const auto value = files_.find(key);
    return value == files_.end() ? nullptr : &value->second;
}

bool Request::has(std::string_view key) const {
    return exists(key);
}

bool Request::exists(std::string_view key) const {
    return body_values_.contains(key) || query_values_.contains(key) || files_.contains(key) ||
           body_arrays_.contains(key) || body_objects_.contains(key);
}

bool Request::is_null(std::string_view key) const {
    const auto body = body_values_.find(key);
    if (body != body_values_.end()) return !body->second.has_value();
    const auto query = query_values_.find(key);
    return query != query_values_.end() && !query->second.has_value();
}

bool Request::filled(std::string_view key) const {
    if (is_array(key)) {
        const auto found = body_array_values_.find(key);
        return found != body_array_values_.end() && !found->second.empty();
    }
    if (is_object(key)) {
        const auto found = body_object_keys_.find(key);
        return found != body_object_keys_.end() && !found->second.empty();
    }
    if (const auto* uploaded = file(key)) return !uploaded->contents.empty();
    const auto value = input(key);
    return value && !value->empty();
}

bool Request::is_array(std::string_view key) const {
    return body_arrays_.contains(key);
}

bool Request::is_object(std::string_view key) const {
    return body_objects_.contains(key);
}

std::vector<std::string> Request::array_values(std::string_view key) const {
    const auto found = body_array_values_.find(key);
    return found == body_array_values_.end() ? std::vector<std::string>{} : found->second;
}

std::vector<std::string> Request::array_keys(std::string_view key) const {
    const auto found = body_object_keys_.find(key);
    return found == body_object_keys_.end() ? std::vector<std::string>{} : found->second;
}

std::map<std::string, std::string> Request::all() const {
    std::map<std::string, std::string> result;
    for (const auto& [key, value] : query_values_)
        if (value) result.insert_or_assign(key, *value);
    for (const auto& [key, value] : body_values_)
        if (value) result.insert_or_assign(key, *value);
    for (const auto& [key, uploaded] : files_)
        result.insert_or_assign(key, uploaded.name);
    return result;
}

const Request& request() {
    if (!active_request)
        throw std::logic_error("request() is only available while handling an HTTP request");
    return *active_request;
}

Session& Request::session() const { return http::session(); }

detail::RequestScope::RequestScope(const Request& current) noexcept
    : previous_(active_request) {
    active_request = &current;
}

detail::RequestScope::~RequestScope() {
    active_request = previous_;
}

} // namespace kotavel::http
