#include "messages.hpp"

#include <stdexcept>
#include <vector>

namespace kotavel {
namespace {
struct Entry {
    std::string_view key;
    std::string_view text;
};

constexpr Entry catalog[] = {
#include "message_catalog.inc"
};

constexpr bool name_start(char character) noexcept {
    return (character >= 'a' && character <= 'z') ||
           (character >= 'A' && character <= 'Z') || character == '_';
}

constexpr bool name_part(char character) noexcept {
    return name_start(character) || (character >= '0' && character <= '9');
}

bool valid_key(std::string_view key) noexcept {
    if (key.empty()) return false;
    std::size_t segment_start = 0;
    while (segment_start < key.size()) {
        const auto separator = key.find('.', segment_start);
        const auto segment_end = separator == std::string_view::npos ? key.size() : separator;
        if (segment_end == segment_start || !name_start(key[segment_start])) return false;
        for (auto index = segment_start + 1; index < segment_end; ++index)
            if (!name_part(key[index])) return false;
        if (separator == std::string_view::npos) return true;
        segment_start = separator + 1;
    }
    return false;
}
}

std::string Messages::get(std::string_view key, std::initializer_list<Parameter> parameters) {
    if (!valid_key(key))
        throw std::invalid_argument("Invalid message key: " + std::string(key));

    std::string_view text;
    bool found = false;
    for (const auto& entry : catalog) {
        if (entry.key == key) {
            text = entry.text;
            found = true;
            break;
        }
    }
    if (!found) throw std::out_of_range("Unknown message key: " + std::string(key));

    for (auto current = parameters.begin(); current != parameters.end(); ++current) {
        if (current->first.empty() || !name_start(current->first.front()))
            throw std::invalid_argument("Invalid message parameter name: " + std::string(current->first));
        for (auto index = std::size_t{1}; index < current->first.size(); ++index) {
            if (!name_part(current->first[index]))
                throw std::invalid_argument("Invalid message parameter name: " + std::string(current->first));
        }
        for (auto previous = parameters.begin(); previous != current; ++previous) {
            if (previous->first == current->first)
                throw std::invalid_argument("Duplicate message parameter: " + std::string(current->first));
        }
    }

    std::string result;
    result.reserve(text.size());
    std::vector<bool> used_parameters(parameters.size(), false);
    // Solo se recorre la plantilla; los valores insertados no se reinterpretan.
    for (std::size_t position = 0; position < text.size();) {
        if (text[position] != ':' || position + 1 == text.size() || !name_start(text[position + 1])) {
            result += text[position++];
            continue;
        }
        const auto start = ++position;
        while (position < text.size() && name_part(text[position])) ++position;
        const auto name = text.substr(start, position - start);
        bool replaced = false;
        std::size_t parameter_index = 0;
        for (const auto& parameter : parameters) {
            if (parameter.first == name) {
                result.append(parameter.second);
                used_parameters[parameter_index] = true;
                replaced = true;
                break;
            }
            ++parameter_index;
        }
        if (!replaced)
            throw std::invalid_argument("Missing message parameter '" + std::string(name) +
                                        "' for key: " + std::string(key));
    }

    std::size_t parameter_index = 0;
    for (const auto& parameter : parameters) {
        if (!used_parameters[parameter_index])
            throw std::invalid_argument("Unused message parameter '" + std::string(parameter.first) +
                                        "' for key: " + std::string(key));
        ++parameter_index;
    }
    return result;
}

} // namespace kotavel
