#include <kotavel/http/api_response.hpp>

#include <rapidjson/stringbuffer.h>
#include <rapidjson/writer.h>

#include <stdexcept>
#include <string>
#include <utility>

namespace kotavel::http {
namespace {
using Writer = rapidjson::Writer<rapidjson::StringBuffer>;

bool write_json(Writer& writer, const JsonValue& value) {
    return std::visit([&writer](const auto& item) -> bool {
        using T = std::remove_cvref_t<decltype(item)>;
        if constexpr (std::is_same_v<T, std::nullptr_t>) {
            return writer.Null();
        } else if constexpr (std::is_same_v<T, bool>) {
            return writer.Bool(item);
        } else if constexpr (std::is_same_v<T, std::int64_t>) {
            return writer.Int64(item);
        } else if constexpr (std::is_same_v<T, std::uint64_t>) {
            return writer.Uint64(item);
        } else if constexpr (std::is_same_v<T, double>) {
            return writer.Double(item);
        } else if constexpr (std::is_same_v<T, std::string>) {
            return writer.String(item.data(), static_cast<rapidjson::SizeType>(item.size()));
        } else if constexpr (std::is_same_v<T, JsonValue::Array>) {
            if (!writer.StartArray()) return false;
            for (const auto& child : item)
                if (!write_json(writer, child)) return false;
            return writer.EndArray(static_cast<rapidjson::SizeType>(item.size()));
        } else {
            if (!writer.StartObject()) return false;
            for (const auto& [key, child] : item) {
                if (!writer.Key(key.data(), static_cast<rapidjson::SizeType>(key.size())) ||
                    !write_json(writer, child)) return false;
            }
            return writer.EndObject(static_cast<rapidjson::SizeType>(item.size()));
        }
    }, value.value());
}

bool write_string(Writer& writer, std::string_view value) {
    return writer.String(value.data(), static_cast<rapidjson::SizeType>(value.size()));
}

} // namespace

Response ApiResponse::success_json(JsonValue data, int status,
                                  std::string_view title, std::string_view message) {
    if (status < 200 || status > 299)
        throw std::invalid_argument("API success status must be between 200 and 299");
    if (status == 204) return Response{204, "application/json; charset=utf-8", {}};

    rapidjson::StringBuffer buffer;
    Writer writer(buffer);
    writer.StartObject();
    writer.Key("data");
    if (!write_json(writer, data)) throw std::runtime_error("Could not serialize JSON response");
    if (!title.empty() || !message.empty()) {
        writer.Key("meta");
        writer.StartObject();
        if (!message.empty()) { writer.Key("message"); write_string(writer, message); }
        if (!title.empty()) { writer.Key("title"); write_string(writer, title); }
        writer.EndObject();
    }
    writer.EndObject();
    return Response{status, "application/json; charset=utf-8", {buffer.GetString(), buffer.GetSize()}};
}

Response ApiResponse::error(int status, const std::vector<ApiError>& errors) {
    if (status < 400 || status > 599)
        throw std::invalid_argument("API error status must be between 400 and 599");
    if (errors.empty()) throw std::invalid_argument("API error response requires at least one error");

    rapidjson::StringBuffer buffer;
    Writer writer(buffer);
    writer.StartObject();
    writer.Key("errors");
    writer.StartArray();
    const auto status_text = std::to_string(status);
    for (const auto& error : errors) {
        writer.StartObject();
        writer.Key("detail"); write_string(writer, error.detail);
        if (error.field) {
            writer.Key("source"); writer.StartObject();
            writer.Key("field"); write_string(writer, *error.field);
            writer.EndObject();
        }
        writer.Key("status"); write_string(writer, status_text);
        writer.Key("title"); write_string(writer, error.title);
        writer.EndObject();
    }
    writer.EndArray();
    writer.EndObject();
    return Response{status, "application/json; charset=utf-8", {buffer.GetString(), buffer.GetSize()}};
}

Response ApiResponse::error(int status, std::string_view title, std::string_view message) {
    return error(status, std::vector<ApiError>{{std::string(title), std::string(message), {}}});
}

} // namespace kotavel::http
