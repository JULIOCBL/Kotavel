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

std::string stringify(const JsonValue& value) {
    rapidjson::StringBuffer buffer;
    Writer writer(buffer);
    if (!write_json(writer, value)) throw std::runtime_error("Could not serialize JSON response");
    return {buffer.GetString(), buffer.GetSize()};
}

} // namespace

Response ApiResponse::success_json(JsonValue data, int status,
                                  std::string_view title, std::string_view message) {
    if (status < 200 || status > 299)
        throw std::invalid_argument("API success status must be between 200 and 299");
    if (status == 204) return Response{204, "application/json; charset=utf-8", {}};

    JsonValue::Object body;
    body.emplace("data", std::move(data));
    if (!title.empty() || !message.empty()) {
        JsonValue::Object meta;
        if (!title.empty()) meta.emplace("title", std::string(title));
        if (!message.empty()) meta.emplace("message", std::string(message));
        body.emplace("meta", std::move(meta));
    }
    return Response{status, "application/json; charset=utf-8", stringify(JsonValue(std::move(body)))};
}

Response ApiResponse::error(int status, const std::vector<ApiError>& errors) {
    if (status < 400 || status > 599)
        throw std::invalid_argument("API error status must be between 400 and 599");
    if (errors.empty()) throw std::invalid_argument("API error response requires at least one error");

    JsonValue::Array encoded_errors;
    encoded_errors.reserve(errors.size());
    const auto status_text = std::to_string(status);
    for (const auto& error : errors) {
        JsonValue::Object item;
        item.emplace("status", status_text);
        item.emplace("title", error.title);
        item.emplace("detail", error.detail);
        if (error.field) item.emplace("source", JsonValue::Object{{"field", *error.field}});
        encoded_errors.emplace_back(std::move(item));
    }

    JsonValue::Object body;
    body.emplace("errors", std::move(encoded_errors));
    return Response{status, "application/json; charset=utf-8", stringify(JsonValue(std::move(body)))};
}

Response ApiResponse::error(int status, std::string_view title, std::string_view message) {
    return error(status, std::vector<ApiError>{{std::string(title), std::string(message), {}}});
}

} // namespace kotavel::http
