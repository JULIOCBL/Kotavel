#include <kotavel/http/form_request.hpp>

#include <rapidjson/document.h>

#include <charconv>
#include <chrono>
#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdlib>
#include <ctime>
#include <fstream>
#include <iomanip>
#include <limits>
#include <filesystem>
#include <regex>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

#if defined(_WIN32)
#include <winsock2.h>
#include <ws2tcpip.h>
#else
#include <netdb.h>
#include <sys/socket.h>
#endif

namespace kotavel::http {
namespace {

std::string escape_html(std::string_view value) {
    std::string escaped;
    for (const char character : value) {
        switch (character) {
            case '&': escaped += "&amp;"; break;
            case '<': escaped += "&lt;"; break;
            case '>': escaped += "&gt;"; break;
            case '"': escaped += "&quot;"; break;
            case '\'': escaped += "&#39;"; break;
            default: escaped.push_back(character); break;
        }
    }
    return escaped;
}

std::pair<std::string_view, std::string_view> rule_parts(std::string_view rule) {
    const auto separator = rule.find(':');
    if (separator == std::string_view::npos) return {rule, {}};
    return {rule.substr(0, separator), rule.substr(separator + 1)};
}

std::vector<std::string_view> split(std::string_view value, char separator = ',') {
    std::vector<std::string_view> parts;
    if (value.empty()) return parts;
    while (true) {
        const auto position = value.find(separator);
        parts.push_back(value.substr(0, position));
        if (position == std::string_view::npos) break;
        value.remove_prefix(position + 1);
    }
    return parts;
}

bool integer(std::string_view value) {
    if (value.empty()) return false;
    long long parsed = 0;
    const auto [end, error] = std::from_chars(value.data(), value.data() + value.size(), parsed);
    return error == std::errc{} && end == value.data() + value.size();
}

std::optional<double> number(std::string_view value);

bool numeric(std::string_view value) {
    if (value.empty()) return false;
    return number(value).has_value();
}

std::optional<double> number(std::string_view value) {
    if (value.empty()) return std::nullopt;
    const std::string owned(value);
    char* end = nullptr;
    const double parsed = std::strtod(owned.c_str(), &end);
    if (end != owned.c_str() + owned.size() || !std::isfinite(parsed)) return std::nullopt;
    return parsed;
}

std::size_t character_count(std::string_view value) {
    std::size_t count = 0;
    for (const unsigned char byte : value)
        if ((byte & 0xC0) != 0x80) ++count;
    return count;
}

bool valid_date(std::string_view value) {
    if (value.size() < 10 || value[4] != '-' || value[7] != '-') return false;
    int year = 0, month = 0, day = 0;
    const auto parse = [](std::string_view part, int& out) {
        const auto [end, error] = std::from_chars(part.data(), part.data() + part.size(), out);
        return error == std::errc{} && end == part.data() + part.size();
    };
    if (!parse(value.substr(0, 4), year) || !parse(value.substr(5, 2), month) ||
        !parse(value.substr(8, 2), day)) return false;
    const std::chrono::year_month_day date{
        std::chrono::year{year}, std::chrono::month{static_cast<unsigned>(month)},
        std::chrono::day{static_cast<unsigned>(day)}};
    if (!date.ok()) return false;
    if (value.size() == 10) return true;
    if (value[10] != 'T' && value[10] != ' ') return false;
    if (value.size() < 19 || value[13] != ':' || value[16] != ':') return false;
    int hour = 0, minute = 0, second = 0;
    if (!parse(value.substr(11, 2), hour) || !parse(value.substr(14, 2), minute) ||
        !parse(value.substr(17, 2), second) || hour > 23 || minute > 59 || second > 60)
        return false;
    for (std::size_t i = 19; i < value.size(); ++i) {
        const char ch = value[i];
        if (!(std::isdigit(static_cast<unsigned char>(ch)) || ch == '.' || ch == '+' ||
              ch == '-' || ch == ':' || ch == 'Z' || ch == 'z')) return false;
    }
    return true;
}

bool valid_date_format(std::string_view value, std::string_view format) {
    // Support the common PHP date() tokens used by Laravel's date_format rule.
    // Literal characters in the format must match exactly.
    std::string expression = "^";
    std::vector<char> captures;
    for (std::size_t i = 0; i < format.size(); ++i) {
        const char token = format[i];
        if (token == '\\' && i + 1 < format.size()) {
            const char literal = format[++i];
            if (std::string_view(".^$|()[]{}*+?\\").find(literal) != std::string_view::npos)
                expression.push_back('\\');
            expression.push_back(literal);
            continue;
        }
        switch (token) {
            case 'Y': expression += "([0-9]{4})"; captures.push_back('Y'); break;
            case 'y': expression += "([0-9]{2})"; captures.push_back('y'); break;
            case 'm': case 'd': case 'H': case 'h': case 'i': case 's':
                expression += "([0-9]{2})"; captures.push_back(token); break;
            case 'n': case 'j': case 'G': case 'g':
                expression += "([0-9]{1,2})"; captures.push_back(token); break;
            default:
                if (std::string_view(".^$|()[]{}*+?\\").find(token) != std::string_view::npos)
                    expression.push_back('\\');
                expression.push_back(token);
        }
    }
    expression += '$';
    try {
        std::smatch match;
        const std::string input(value);
        if (!std::regex_match(input, match, std::regex(expression))) return false;
        int parsed_year = 2000, parsed_month = 1, parsed_day = 1, hour = 0, minute = 0, second = 0;
        for (std::size_t i = 0; i < captures.size(); ++i) {
            int parsed = std::stoi(match[i + 1].str());
            switch (captures[i]) {
                case 'Y': parsed_year = parsed; break;
                case 'y': parsed_year = parsed < 70 ? 2000 + parsed : 1900 + parsed; break;
                case 'm': case 'n': parsed_month = parsed; break;
                case 'd': case 'j': parsed_day = parsed; break;
                case 'H': case 'h': case 'G': case 'g': hour = parsed; break;
                case 'i': minute = parsed; break;
                case 's': second = parsed; break;
            }
        }
        if (!(std::chrono::year_month_day{
                  std::chrono::year{parsed_year},
                  std::chrono::month{static_cast<unsigned>(parsed_month)},
                  std::chrono::day{static_cast<unsigned>(parsed_day)}}).ok()) return false;
        return hour <= 23 && minute <= 59 && second <= 59;
    } catch (...) {
        return false;
    }
}

bool valid_timezone(std::string_view value, std::string_view region = {}) {
    if (value == "UTC" || value == "Etc/UTC" || value == "GMT") return true;
    if (value.empty() || value.starts_with('/') || value.find("..") != std::string_view::npos ||
        (!region.empty() && !value.starts_with(std::string(region) + "/"))) return false;
    const std::filesystem::path zone("/usr/share/zoneinfo");
    std::error_code error;
    return std::filesystem::is_regular_file(zone / std::string(value), error) && !error;
}

bool timezone_is_in_country(std::string_view value, std::string_view country) {
    if (country.size() != 2 || !valid_timezone(value)) return false;
    std::ifstream zones("/usr/share/zoneinfo/zone.tab");
    std::string line;
    while (std::getline(zones, line)) {
        if (line.empty() || line.front() == '#') continue;
        const auto first_tab = line.find('\t');
        if (first_tab == std::string::npos) continue;
        const auto second_tab = line.find('\t', first_tab + 1);
        if (second_tab == std::string::npos) continue;
        if (std::string_view(line).substr(0, first_tab) != country) continue;
        const auto zone_end = line.find('\t', second_tab + 1);
        const auto zone = std::string_view(line).substr(second_tab + 1,
            zone_end == std::string::npos ? line.size() - second_tab - 1 : zone_end - second_tab - 1);
        if (zone == value) return true;
    }
    return false;
}

bool valid_ip(std::string_view value, int version = 0) {
    if (value.empty()) return false;
    if (value.find(':') == std::string_view::npos) {
        if (version == 6) return false;
        const auto pieces = split(value, '.');
        if (pieces.size() != 4) return false;
        for (const auto piece : pieces) {
            int octet = -1;
            const auto [end, error] = std::from_chars(piece.data(), piece.data() + piece.size(), octet);
            if (piece.empty() || error != std::errc{} || end != piece.data() + piece.size() ||
                octet < 0 || octet > 255) return false;
        }
        return true;
    }
    if (version == 4) return false;
    if (value.find_first_not_of("0123456789abcdefABCDEF:") != std::string_view::npos)
        return false;
    const auto compression = value.find("::");
    if (compression != std::string_view::npos && value.find("::", compression + 2) != std::string_view::npos)
        return false;
    const auto pieces = split(value, ':');
    std::size_t groups = 0;
    for (const auto piece : pieces) {
        if (piece.empty()) continue;
        if (piece.size() > 4) return false;
        ++groups;
    }
    return compression == std::string_view::npos ? groups == 8 : groups < 8;
}

bool valid_url(std::string_view value) {
    const auto scheme = value.find("://");
    if (scheme == std::string_view::npos || scheme == 0) return false;
    for (const unsigned char ch : value.substr(0, scheme))
        if (!std::isalnum(ch) && ch != '+' && ch != '-' && ch != '.') return false;
    const auto host_start = scheme + 3;
    const auto host_end = value.find_first_of("/?#", host_start);
    auto authority = value.substr(host_start, host_end == std::string_view::npos
        ? value.size() - host_start : host_end - host_start);
    if (const auto at = authority.rfind('@'); at != std::string_view::npos)
        authority.remove_prefix(at + 1);
    if (authority.empty()) return false;
    if (authority.front() == '[') {
        const auto close = authority.find(']');
        if (close <= 1 || !valid_ip(authority.substr(1, close - 1), 6)) return false;
        if (close + 1 == authority.size()) return true;
        if (authority[close + 1] != ':') return false;
        authority.remove_prefix(close + 2);
        int port = 0;
        const auto [end, error] = std::from_chars(authority.data(), authority.data() + authority.size(), port);
        return !authority.empty() && error == std::errc{} && end == authority.data() + authority.size() &&
               port > 0 && port <= 65535;
    }
    if (const auto colon = authority.rfind(':'); colon != std::string_view::npos) {
        const auto port_text = authority.substr(colon + 1);
        int port = 0;
        const auto [end, error] = std::from_chars(port_text.data(), port_text.data() + port_text.size(), port);
        if (port_text.empty() || error != std::errc{} || end != port_text.data() + port_text.size() ||
            port <= 0 || port > 65535) return false;
        authority = authority.substr(0, colon);
    }
    if (authority.back() == '.') authority.remove_suffix(1);
    if (authority.empty() || authority.front() == '.' || authority.find("..") != std::string_view::npos)
        return false;
    for (const auto label : split(authority, '.')) {
        if (label.empty() || label.size() > 63 || label.front() == '-' || label.back() == '-') return false;
        for (const unsigned char ch : label)
            if (!std::isalnum(ch) && ch != '-') return false;
    }
    return true;
}

bool resolves_host(std::string_view host) {
    const std::string name(host);
    addrinfo hints{};
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_family = AF_UNSPEC;
    addrinfo* results = nullptr;
    const int status = getaddrinfo(name.c_str(), nullptr, &hints, &results);
    if (results) freeaddrinfo(results);
    return status == 0;
}

std::string_view url_host(std::string_view value) {
    const auto scheme = value.find("://");
    if (scheme == std::string_view::npos) return {};
    auto authority = value.substr(scheme + 3);
    if (const auto end = authority.find_first_of("/?#"); end != std::string_view::npos)
        authority = authority.substr(0, end);
    if (const auto at = authority.rfind('@'); at != std::string_view::npos)
        authority.remove_prefix(at + 1);
    if (authority.starts_with("[")) {
        const auto close = authority.find(']');
        return close == std::string_view::npos ? std::string_view{} : authority.substr(1, close - 1);
    }
    if (const auto colon = authority.find(':'); colon != std::string_view::npos)
        authority = authority.substr(0, colon);
    return authority;
}

bool in_values(std::string_view value, std::string_view argument) {
    for (const auto candidate : split(argument)) if (value == candidate) return true;
    return false;
}

bool begins_or_ends(std::string_view value, std::string_view argument, bool prefix) {
    for (const auto candidate : split(argument)) {
        if (candidate.size() > value.size()) continue;
        if (prefix ? value.starts_with(candidate) : value.ends_with(candidate)) return true;
    }
    return false;
}

bool regex_match(std::string_view value, std::string_view expression) {
    try {
        std::string pattern(expression);
        std::regex::flag_type flags = std::regex::ECMAScript;
        if (pattern.size() >= 2 && pattern.front() == '/') {
            const auto last = pattern.rfind('/');
            if (last > 0) {
                const auto modifiers = pattern.substr(last + 1);
                pattern = pattern.substr(1, last - 1);
                if (modifiers.find('i') != std::string::npos) flags |= std::regex::icase;
            }
        }
        return std::regex_match(value.begin(), value.end(), std::regex(pattern, flags));
    } catch (const std::regex_error&) {
        return false;
    }
}

std::string lowercase(std::string value) {
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    return value;
}

std::string file_extension(std::string_view name) {
    const auto dot = name.find_last_of('.');
    return dot == std::string_view::npos ? std::string{} : lowercase(std::string(name.substr(dot + 1)));
}

std::string detected_mime(const UploadedFile& file) {
    auto bytes = std::string_view(file.contents);
    while (!bytes.empty() && std::isspace(static_cast<unsigned char>(bytes.front()))) bytes.remove_prefix(1);
    const auto sample = lowercase(std::string(bytes.substr(0, std::min<std::size_t>(bytes.size(), 512))));
    if (sample.starts_with("<svg") ||
        (sample.starts_with("<?xml") && sample.find("<svg") != std::string::npos)) return "image/svg+xml";
    if (bytes.starts_with("\x89PNG\r\n\x1a\n")) return "image/png";
    if (bytes.starts_with("\xff\xd8\xff")) return "image/jpeg";
    if (bytes.starts_with("GIF87a") || bytes.starts_with("GIF89a")) return "image/gif";
    if (bytes.starts_with("BM")) return "image/bmp";
    if (bytes.starts_with("%PDF-")) return "application/pdf";
    if (bytes.size() >= 12 && bytes.substr(0, 4) == "RIFF" && bytes.substr(8, 4) == "WEBP")
        return "image/webp";
    if (bytes.size() >= 4 && static_cast<unsigned char>(bytes[0]) == 0x50 &&
        static_cast<unsigned char>(bytes[1]) == 0x4b) return "application/zip";
    if (bytes.find('\0') == std::string_view::npos) return "text/plain";
    return "application/octet-stream";
}

std::optional<std::pair<std::uint32_t, std::uint32_t>> image_dimensions(const UploadedFile& file) {
    const std::string_view bytes(file.contents);
    const auto be32 = [&](std::size_t offset) -> std::uint32_t {
        return (static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[offset])) << 24) |
            (static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[offset + 1])) << 16) |
            (static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[offset + 2])) << 8) |
            static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[offset + 3]));
    };
    const auto le32 = [&](std::size_t offset) -> std::uint32_t {
        return static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[offset])) |
            (static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[offset + 1])) << 8) |
            (static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[offset + 2])) << 16) |
            (static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[offset + 3])) << 24);
    };
    if (bytes.starts_with("\x89PNG\r\n\x1a\n") && bytes.size() >= 24)
        return std::pair{be32(16), be32(20)};
    if ((bytes.starts_with("GIF87a") || bytes.starts_with("GIF89a")) && bytes.size() >= 10) {
        const auto width = static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[6])) |
            (static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[7])) << 8);
        const auto height = static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[8])) |
            (static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[9])) << 8);
        return std::pair{width, height};
    }
    if (bytes.starts_with("BM") && bytes.size() >= 26)
        return std::pair{le32(18), le32(22)};
    if (bytes.starts_with("\xff\xd8")) {
        std::size_t offset = 2;
        while (offset + 4 < bytes.size()) {
            if (static_cast<unsigned char>(bytes[offset]) != 0xff) break;
            const auto marker = static_cast<unsigned char>(bytes[offset + 1]);
            const auto size = (static_cast<std::size_t>(static_cast<unsigned char>(bytes[offset + 2])) << 8) |
                static_cast<unsigned char>(bytes[offset + 3]);
            if (size < 2 || offset + 2 + size > bytes.size()) break;
            if ((marker >= 0xc0 && marker <= 0xc3) || (marker >= 0xc5 && marker <= 0xc7) ||
                (marker >= 0xc9 && marker <= 0xcb) || (marker >= 0xcd && marker <= 0xcf)) {
                const auto height = (static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[offset + 5])) << 8) |
                    static_cast<unsigned char>(bytes[offset + 6]);
                const auto width = (static_cast<std::uint32_t>(static_cast<unsigned char>(bytes[offset + 7])) << 8) |
                    static_cast<unsigned char>(bytes[offset + 8]);
                return std::pair{width, height};
            }
            offset += 2 + size;
        }
    }
    return std::nullopt;
}

bool valid_utf8(std::string_view value) {
    std::size_t i = 0;
    while (i < value.size()) {
        const auto lead = static_cast<unsigned char>(value[i]);
        std::size_t continuation = lead < 0x80 ? 0 : (lead & 0xE0) == 0xC0 ? 1 :
            (lead & 0xF0) == 0xE0 ? 2 : (lead & 0xF8) == 0xF0 ? 3 : 99;
        if (continuation == 99 || i + continuation >= value.size()) return false;
        if ((continuation == 1 && lead < 0xC2) || (continuation == 3 && lead > 0xF4)) return false;
        for (std::size_t j = 1; j <= continuation; ++j)
            if ((static_cast<unsigned char>(value[i + j]) & 0xC0) != 0x80) return false;
        if (continuation == 2) {
            const auto second = static_cast<unsigned char>(value[i + 1]);
            if ((lead == 0xE0 && second < 0xA0) || (lead == 0xED && second >= 0xA0)) return false;
        }
        if (continuation == 3) {
            const auto second = static_cast<unsigned char>(value[i + 1]);
            if ((lead == 0xF0 && second < 0x90) || (lead == 0xF4 && second >= 0x90)) return false;
        }
        i += continuation + 1;
    }
    return true;
}

bool valid_encoding(std::string_view value, std::string_view encoding) {
    const auto normalized = lowercase(std::string(encoding));
    if (normalized == "utf-8" || normalized == "utf8") return valid_utf8(value);
    if (normalized == "ascii" || normalized == "us-ascii")
        return std::all_of(value.begin(), value.end(), [](unsigned char ch) { return ch < 0x80; });
    if (normalized == "iso-8859-1" || normalized == "latin1" ||
        normalized == "windows-1252" || normalized == "cp1252") return true;
    if (normalized == "utf-16le" || normalized == "utf-16be") {
        if (value.size() % 2 != 0) return false;
        const bool little = normalized.ends_with("le");
        for (std::size_t i = 0; i < value.size(); i += 2) {
            const auto high = static_cast<unsigned char>(value[i + (little ? 1 : 0)]);
            if (high >= 0xD8 && high <= 0xDB) {
                if (i + 3 >= value.size()) return false;
                const auto next_high = static_cast<unsigned char>(value[i + 2 + (little ? 1 : 0)]);
                if (next_high < 0xDC || next_high > 0xDF) return false;
                i += 2;
            } else if (high >= 0xDC && high <= 0xDF) return false;
        }
        return true;
    }
    return false;
}

bool accepts(const Request& request, std::string_view field, std::string_view value,
             std::string_view rule, std::string_view argument,
             bool numeric_attribute) {
    const auto other = request.input(argument);
    const auto* upload = request.file(field);
    if (rule == "required") return !value.empty();
    if (rule == "nullable") return true;
    if (rule == "string") return !request.is_array(field) && !request.is_object(field);
    if (rule == "array") {
        if (!request.is_array(field) && !request.is_object(field)) return false;
        if (argument.empty()) return true;
        for (const auto key : request.array_keys(field))
            if (!in_values(key, argument)) return false;
        return true;
    }
    if (rule == "list") return request.is_array(field) &&
        request.array_keys(field).empty(); // JSON arrays use sequential indexes.
    if (rule == "integer") return integer(value);
    if (rule == "numeric") return numeric(value);
    if (rule == "boolean") return argument == "strict"
        ? value == "true" || value == "false"
        : value == "0" || value == "1" || value == "true" || value == "false";
    if (rule == "accepted") return in_values(value, "yes,on,1,true");
    if (rule == "declined") return in_values(value, "no,off,0,false");
    if (rule == "email") {
        const auto at = value.find('@');
        const auto domain = at == std::string_view::npos ? std::string_view{} : value.substr(at + 1);
        const bool syntax = at != std::string_view::npos && at > 0 && at + 1 < value.size() &&
            domain.find('.') != std::string_view::npos && domain.find("..") == std::string_view::npos &&
            value.find(' ', 0) == std::string_view::npos;
        if (!syntax) return false;
        const auto styles = split(argument);
        if (std::find(styles.begin(), styles.end(), "dns") != styles.end())
            return resolves_host(domain);
        return styles.empty() || std::all_of(styles.begin(), styles.end(), [](auto style) {
            return style == "rfc" || style == "strict" || style == "filter";
        });
    }
    if (rule == "file") return upload != nullptr;
    if (rule == "image") return upload && detected_mime(*upload).starts_with("image/") &&
        (detected_mime(*upload) != "image/svg+xml" || in_values("allow_svg", argument));
    if (rule == "mimes") {
        if (!upload) return false;
        const auto mime = detected_mime(*upload);
        static const std::map<std::string, std::string> extension_mimes{
            {"jpg", "image/jpeg"}, {"jpeg", "image/jpeg"}, {"png", "image/png"},
            {"gif", "image/gif"}, {"bmp", "image/bmp"}, {"webp", "image/webp"},
            {"pdf", "application/pdf"}, {"txt", "text/plain"}, {"csv", "text/plain"},
            {"zip", "application/zip"}, {"json", "application/json"},
        };
        for (const auto extension : split(argument)) {
            const auto found = extension_mimes.find(std::string(extension));
            if (found != extension_mimes.end() && found->second == mime) return true;
        }
        return false;
    }
    if (rule == "mimetypes") return upload && in_values(detected_mime(*upload), argument);
    if (rule == "extensions") return upload && in_values(file_extension(upload->name), argument);
    if (rule == "encoding")
        return valid_encoding(upload ? std::string_view(upload->contents) : value, argument);
    if (rule == "dimensions") {
        if (!upload) return false;
        const auto dimensions = image_dimensions(*upload);
        if (!dimensions) return false;
        for (const auto constraint : split(argument)) {
            const auto equal = constraint.find('=');
            if (equal == std::string_view::npos) return false;
            const auto name = constraint.substr(0, equal);
            const auto expected_text = constraint.substr(equal + 1);
            const double width = dimensions->first, height = dimensions->second;
            if (name == "ratio") {
                const auto parts = split(expected_text, '/');
                if (parts.empty() || parts.front().empty()) return false;
                const auto numerator = number(parts.front());
                const auto denominator = parts.size() == 2 ? number(parts.back()) : std::optional<double>{1.0};
                if (!numerator || !denominator || *denominator == 0 ||
                    std::abs(width / height - *numerator / *denominator) > 0.01) return false;
                continue;
            }
            const auto expected = number(expected_text);
            if (!expected) return false;
            if (name == "min_width" && width < *expected) return false;
            if (name == "max_width" && width > *expected) return false;
            if (name == "width" && width != *expected) return false;
            if (name == "min_height" && height < *expected) return false;
            if (name == "max_height" && height > *expected) return false;
            if (name == "height" && height != *expected) return false;
        }
        return true;
    }
    if (rule == "url") return valid_url(value);
    if (rule == "active_url") return valid_url(value) && resolves_host(url_host(value));
    if (rule == "ip") return valid_ip(value);
    if (rule == "ipv4") return valid_ip(value, 4);
    if (rule == "ipv6") return valid_ip(value, 6);
    if (rule == "json") {
        rapidjson::Document document;
        document.Parse(value.data(), value.size());
        return !document.HasParseError();
    }
    if (rule == "ascii")
        return std::all_of(value.begin(), value.end(), [](unsigned char ch) { return ch < 0x80; });
    if (rule == "alpha" || rule == "alpha_num" || rule == "alpha_dash") {
        if (value.empty()) return false;
        for (const unsigned char ch : value) {
            if (std::isalpha(ch) || (argument != "ascii" && ch >= 0x80)) continue;
            if (rule != "alpha" && std::isdigit(ch)) continue;
            if (rule == "alpha_dash" && (ch == '_' || ch == '-')) continue;
            return false;
        }
        return true;
    }
    if (rule == "lowercase") {
        return std::none_of(value.begin(), value.end(), [](unsigned char ch) {
            return std::isalpha(ch) && std::isupper(ch);
        });
    }
    if (rule == "uppercase") {
        return std::none_of(value.begin(), value.end(), [](unsigned char ch) {
            return std::isalpha(ch) && std::islower(ch);
        });
    }
    if (rule == "in") return in_values(value, argument);
    if (rule == "not_in") return !in_values(value, argument);
    if (rule == "enum") return in_values(value, argument);
    if (rule == "any_of") {
        for (const auto candidate : split(argument, ';')) {
            const auto [candidate_rule, candidate_argument] = rule_parts(candidate);
            if (accepts(request, field, value, candidate_rule, candidate_argument, numeric_attribute))
                return true;
        }
        return false;
    }
    if (rule == "same") return other && value == *other;
    if (rule == "different") return !other || value != *other;
    if (rule == "confirmed") {
        const auto confirmation = request.input(argument.empty() ? std::string(field) + "_confirmation"
                                                                  : std::string(argument));
        return confirmation && value == *confirmation;
    }
    if (rule == "starts_with") return begins_or_ends(value, argument, true);
    if (rule == "ends_with") return begins_or_ends(value, argument, false);
    if (rule == "doesnt_start_with") return !begins_or_ends(value, argument, true);
    if (rule == "doesnt_end_with") return !begins_or_ends(value, argument, false);
    if (rule == "regex") return regex_match(value, argument);
    if (rule == "not_regex") return !regex_match(value, argument);
    if (rule == "hex_color") return regex_match(value, R"(^#?([0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$)");
    if (rule == "mac_address") return regex_match(value, R"(^([0-9a-fA-F]{2}[:-]){5}[0-9a-fA-F]{2}$)");
    if (rule == "uuid") return regex_match(value, R"(^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$)");
    if (rule == "ulid") return regex_match(value, R"(^[0-7][0-9A-HJKMNP-TV-Z]{25}$)");
    if (rule == "date") return valid_date(value);
    if (rule == "date_format") {
        for (const auto format : split(argument)) if (valid_date_format(value, format)) return true;
        return false;
    }
    if (rule == "date_equals" || rule == "after" || rule == "after_or_equal" ||
        rule == "before" || rule == "before_or_equal") {
        auto comparable = argument;
        if (const auto reference = request.input(argument)) comparable = *reference;
        if (!valid_date(value) || !valid_date(comparable)) return false;
        if (rule == "date_equals") return value == comparable;
        if (rule == "after") return value > comparable;
        if (rule == "after_or_equal") return value >= comparable;
        if (rule == "before") return value < comparable;
        return value <= comparable;
    }
    if (rule == "timezone") {
        const auto args = split(argument);
        if (args.empty() || args.front() == "all") return valid_timezone(value);
        if (args.front() == "per_country")
            return args.size() == 2 && timezone_is_in_country(value, args[1]);
        return valid_timezone(value, args.front());
    }
    if (rule == "min" || rule == "max" || rule == "size" || rule == "between") {
        const auto bounds = split(argument);
        if (rule == "between" && bounds.size() != 2) return false;
        if (bounds.empty()) return false;
        const auto lower_bound = number(bounds.front());
        const auto upper_bound = rule == "between" ? number(bounds.back()) : std::optional<double>{};
        if (!lower_bound || (rule == "between" && !upper_bound)) return false;
        double size = 0;
        if (upload) size = static_cast<double>(upload->contents.size()) / 1024.0;
        else if (request.is_array(field) || request.is_object(field))
            size = static_cast<double>(request.is_array(field) ? request.array_values(field).size()
                                                               : request.array_keys(field).size());
        else if (numeric_attribute) {
            const auto parsed = number(value);
            if (!parsed) return false;
            size = *parsed;
        } else size = static_cast<double>(character_count(value));
        if (rule == "min") return size >= *lower_bound;
        if (rule == "max") return size <= *lower_bound;
        if (rule == "size") return size == *lower_bound;
        return size >= *lower_bound && size <= *upper_bound;
    }
    if (rule == "decimal") {
        if (!numeric(value)) return false;
        const auto point = value.find('.');
        const auto decimals = point == std::string_view::npos ? 0 : value.size() - point - 1;
        const auto bounds = split(argument);
        if (bounds.empty()) return false;
        const auto minimum = number(bounds.front());
        const auto maximum = bounds.size() > 1 ? number(bounds[1]) : minimum;
        return minimum && maximum && decimals >= *minimum && decimals <= *maximum;
    }
    if (rule == "digits" || rule == "digits_between" || rule == "min_digits" || rule == "max_digits") {
        if (!std::all_of(value.begin(), value.end(), [](unsigned char ch) { return std::isdigit(ch); })) return false;
        const auto bounds = split(argument);
        const double length = static_cast<double>(value.size());
        if (bounds.empty()) return false;
        const auto first = number(bounds.front());
        if (!first) return false;
        if (rule == "digits") return length == *first;
        if (rule == "min_digits") return length >= *first;
        if (rule == "max_digits") return length <= *first;
        const auto last = bounds.size() > 1 ? number(bounds[1]) : std::nullopt;
        return last && length >= *first && length <= *last;
    }
    if (rule == "gt" || rule == "gte" || rule == "lt" || rule == "lte" || rule == "multiple_of") {
        const auto left = number(value);
        auto right = number(argument);
        if (!right) if (const auto reference = request.input(argument)) right = number(*reference);
        if (!left || !right) return false;
        if (rule == "gt") return *left > *right;
        if (rule == "gte") return *left >= *right;
        if (rule == "lt") return *left < *right;
        if (rule == "lte") return *left <= *right;
        return *right != 0 && std::fmod(*left, *right) == 0;
    }
    if (rule == "array_keys") {
        const auto keys = request.array_keys(field);
        if (keys.empty()) return false;
        for (const auto key : keys)
            if (!in_values(key, argument)) return false;
        return true;
    }
    if (rule == "required_array_keys") {
        const auto keys = request.array_keys(field);
        for (const auto key : split(argument))
            if (std::find(keys.begin(), keys.end(), key) == keys.end()) return false;
        return true;
    }
    if (rule == "contains" || rule == "doesnt_contain" || rule == "distinct") {
        const auto values = request.array_values(field);
        if (rule == "contains") {
            for (const auto expected : split(argument))
                if (std::find(values.begin(), values.end(), expected) == values.end()) return false;
            return true;
        }
        if (rule == "doesnt_contain") {
            for (const auto forbidden : split(argument))
                if (std::find(values.begin(), values.end(), forbidden) != values.end()) return false;
            return true;
        }
        auto sorted = values;
        if (argument.find("ignore_case") != std::string_view::npos)
            for (auto& item : sorted) std::transform(item.begin(), item.end(), item.begin(),
                [](unsigned char ch) { return static_cast<char>(std::tolower(ch)); });
        std::sort(sorted.begin(), sorted.end());
        return std::adjacent_find(sorted.begin(), sorted.end()) == sorted.end();
    }
    if (rule == "in_array") {
        if (const auto wildcard = argument.find(".*"); wildcard != std::string_view::npos) {
            const auto values = request.array_values(argument.substr(0, wildcard));
            return std::find(values.begin(), values.end(), value) != values.end();
        }
        const auto values = request.array_values(argument);
        return std::find(values.begin(), values.end(), value) != values.end() ||
               (other && *other == value);
    }
    if (rule == "in_array_keys") {
        const auto keys = request.array_keys(field);
        const auto expected = split(argument);
        return std::any_of(expected.begin(), expected.end(), [&](auto key) {
            return std::find(keys.begin(), keys.end(), key) != keys.end();
        });
    }
    if (rule == "bail" || rule == "sometimes" || rule == "exclude" || rule == "exclude_if" ||
        rule == "exclude_unless" || rule == "exclude_with" || rule == "exclude_without" ||
        rule == "filled" || rule == "missing" || rule == "missing_if" || rule == "missing_unless" ||
        rule == "missing_with" || rule == "missing_with_all" || rule == "present" ||
        rule == "present_if" || rule == "present_unless" || rule == "present_with" ||
        rule == "present_with_all" || rule == "prohibited" || rule == "prohibited_if" ||
        rule == "prohibited_if_accepted" || rule == "prohibited_if_declined" ||
        rule == "prohibited_unless" || rule == "prohibits" || rule == "required_if" ||
        rule == "required_if_accepted" || rule == "required_if_declined" || rule == "required_unless" ||
        rule == "required_with" || rule == "required_with_all" || rule == "required_without" ||
        rule == "required_without_all" || rule == "required_without" || rule == "sometimes" ||
        rule == "accepted_if" || rule == "declined_if" || rule == "any_of" || rule == "nullable") return true;
    throw std::invalid_argument("Unsupported FormRequest validation rule: " + std::string(rule));
}

bool matches_condition(const Request& request, std::string_view field,
                       std::string_view rule, std::string_view argument) {
    const auto args = split(argument);
    if (args.empty()) return false;
    const auto other = request.input(args.front());
    const bool other_present = request.exists(args.front());
    const bool other_filled = request.filled(args.front());
    const auto compares = [&](std::string_view value) {
        return std::find(args.begin() + 1, args.end(), value) != args.end();
    };
    if (rule == "required_if_accepted" || rule == "prohibited_if_accepted")
        return other && in_values(*other, "yes,on,1,true");
    if (rule == "required_if_declined" || rule == "prohibited_if_declined")
        return other && in_values(*other, "no,off,0,false");
    if (rule == "accepted_if" || rule == "declined_if")
        return other && compares(*other);
    if (rule.ends_with("_if")) return other && compares(*other);
    if (rule.ends_with("_unless")) return !other || !compares(*other);
    if (rule == "present_with" || rule == "missing_with") {
        return std::any_of(args.begin(), args.end(), [&](auto name) {
            return name != field && request.exists(name);
        });
    }
    if (rule == "exclude_with" || rule == "required_with") {
        return std::any_of(args.begin(), args.end(), [&](auto name) {
            return name != field && request.filled(name);
        });
    }
    if (rule == "exclude_without" || rule == "required_without" || rule == "present_without" ||
        rule == "missing_without") {
        return std::any_of(args.begin(), args.end(), [&](auto name) {
            return name != field && !request.filled(name);
        });
    }
    if (rule == "present_with_all" || rule == "missing_with_all") {
        return std::all_of(args.begin(), args.end(), [&](auto name) {
            return name != field && request.exists(name);
        });
    }
    if (rule == "required_with_all") {
        return std::all_of(args.begin(), args.end(), [&](auto name) {
            return name != field && request.filled(name);
        });
    }
    if (rule == "required_without_all" || rule == "missing_without_all") {
        return std::all_of(args.begin(), args.end(), [&](auto name) {
            return name != field && !request.filled(name);
        });
    }
    if (rule == "exclude" || rule == "required" || rule == "present" || rule == "missing" ||
        rule == "filled" || rule == "prohibited" || rule == "prohibits") return true;
    (void)other_present;
    (void)other_filled;
    return false;
}

bool conditional_rule_passes(const Request& request, std::string_view field,
                             std::string_view rule, std::string_view argument) {
    const auto args = split(argument);
    const bool present = request.exists(field);
    const bool filled = request.filled(field);
    const auto other = args.empty() ? std::optional<std::string_view>{} : request.input(args.front());
    const auto matches_other = [&] {
        if (!other) return false;
        return std::find(args.begin() + (args.empty() ? 0 : 1), args.end(), *other) != args.end();
    };
    const bool condition = matches_condition(request, field, rule, argument);
    if (rule == "required") return filled;
    if (rule == "present") return present;
    if (rule == "missing") return !present;
    if (rule == "filled") return !present || filled;
    if (rule == "required_if" || rule == "required_if_accepted" || rule == "required_if_declined")
        return !condition || filled;
    if (rule == "required_unless") return !condition || filled;
    if (rule == "required_with" || rule == "required_with_all" ||
        rule == "required_without" || rule == "required_without_all") return !condition || filled;
    if (rule == "accepted_if") return !condition || (request.input(field) &&
        in_values(*request.input(field), "yes,on,1,true"));
    if (rule == "declined_if") return !condition || (request.input(field) &&
        in_values(*request.input(field), "no,off,0,false"));
    if (rule == "present_if" || rule == "present_unless" || rule == "present_with" ||
        rule == "present_with_all") return !condition || present;
    if (rule == "missing_if" || rule == "missing_unless" || rule == "missing_with" ||
        rule == "missing_with_all") return !condition || !present;
    if (rule == "prohibited") return !filled;
    if (rule == "prohibited_if" || rule == "prohibited_if_accepted" ||
        rule == "prohibited_if_declined" || rule == "prohibited_unless") return !condition || !filled;
    if (rule == "prohibits") {
        if (!filled) return true;
        return std::all_of(args.begin(), args.end(), [&](auto name) {
            return name == field || !request.filled(name);
        });
    }
    if (rule == "in_array") return true;
    (void)matches_other;
    return true;
}

bool excludes_field(const Request& request, std::string_view field,
                    std::string_view rule, std::string_view argument) {
    if (rule == "exclude") return true;
    if (rule != "exclude_if" && rule != "exclude_unless" &&
        rule != "exclude_with" && rule != "exclude_without") return false;
    return matches_condition(request, field, rule, argument);
}

bool path_matches(std::string_view pattern, std::string_view key) {
    const auto pattern_parts = split(pattern, '.');
    const auto key_parts = split(key, '.');
    if (pattern_parts.size() != key_parts.size()) return false;
    for (std::size_t index = 0; index < pattern_parts.size(); ++index)
        if (pattern_parts[index] != "*" && pattern_parts[index] != key_parts[index]) return false;
    return true;
}

std::string message_for(std::string_view field, std::string_view rule,
                        std::string_view argument) {
    const auto label = std::string(field);
    if (rule == "required") return "The " + label + " field is required.";
    if (rule == "string") return "The " + label + " field must be a string.";
    if (rule == "integer") return "The " + label + " field must be an integer.";
    if (rule == "numeric") return "The " + label + " field must be a number.";
    if (rule == "boolean") return "The " + label + " field must be true or false.";
    if (rule == "email") return "The " + label + " field must be a valid email address.";
    if (rule == "min") return "The " + label + " field must be at least " + std::string(argument) + ".";
    if (rule == "max") return "The " + label + " field may not be greater than " + std::string(argument) + ".";
    if (rule == "in") return "The selected " + label + " is invalid.";
    return "The " + label + " field is invalid.";
}

} // namespace

bool FormRequest::validate() {
    validation_ran_ = true;
    authorized_ = authorize();
    validated_.clear();
    errors_.clear();
    if (!authorized_) return false;
    if (!has_valid_input()) {
        errors_.push_back({"Validation Failed", "The request body contains invalid JSON.", {}});
        return false;
    }

    const auto declared_rules = rules();
    ValidationRules expanded_rules;
    const auto request_values = all();
    for (const auto& [pattern, field_rules] : declared_rules) {
        if (pattern.find('*') == std::string::npos) {
            expanded_rules.emplace(pattern, field_rules);
            continue;
        }
        for (const auto& [key, ignored] : request_values) {
            (void)ignored;
            if (path_matches(pattern, key)) expanded_rules.insert_or_assign(key, field_rules);
        }
    }

    for (const auto& [field, field_rules] : expanded_rules) {
        bool excluded = false;
        for (const auto& encoded_rule : field_rules) {
            const auto [rule, argument] = rule_parts(encoded_rule);
            if (excludes_field(*this, field, rule, argument)) excluded = true;
        }
        if (excluded) continue;

        const bool present = exists(field);
        const bool composite = is_array(field) || is_object(field);
        auto value = input(field);
        if (composite && !value) value = std::string_view{};
        const bool nullable = std::find(field_rules.begin(), field_rules.end(), "nullable") != field_rules.end();
        const bool sometimes = std::find(field_rules.begin(), field_rules.end(), "sometimes") != field_rules.end();
        if (!present && sometimes) continue;

        const bool numeric_attribute = std::any_of(field_rules.begin(), field_rules.end(), [](const auto& encoded) {
            const auto [rule, argument] = rule_parts(encoded);
            (void)argument;
            return rule == "numeric" || rule == "integer" || rule == "decimal";
        });
        bool field_valid = true;
        bool stop_field = false;
        for (const auto& encoded_rule : field_rules) {
            const auto [rule, argument] = rule_parts(encoded_rule);
            if (rule == "nullable" || rule == "sometimes" || rule == "exclude" ||
                rule == "exclude_if" || rule == "exclude_unless" || rule == "exclude_with" ||
                rule == "exclude_without") continue;
            const bool is_conditional = rule == "required" || rule == "required_if" ||
                rule == "required_if_accepted" || rule == "required_if_declined" ||
                rule == "required_unless" || rule == "required_with" || rule == "required_with_all" ||
                rule == "required_without" || rule == "required_without_all" || rule == "present" ||
                rule == "present_if" || rule == "present_unless" || rule == "present_with" ||
                rule == "present_with_all" || rule == "missing" || rule == "missing_if" ||
                rule == "missing_unless" || rule == "missing_with" || rule == "missing_with_all" ||
                rule == "filled" || rule == "prohibited" || rule == "prohibited_if" ||
                rule == "prohibited_if_accepted" || rule == "prohibited_if_declined" ||
                rule == "prohibited_unless" || rule == "prohibits" || rule == "accepted_if" ||
                rule == "declined_if";
            if (is_conditional) {
                if (!conditional_rule_passes(*this, field, rule, argument)) {
                    errors_.push_back({"Validation Failed", message_for(field, rule, argument), field});
                    field_valid = false;
                    if (std::find(field_rules.begin(), field_rules.end(), "bail") != field_rules.end()) break;
                }
                continue;
            }
            if (rule == "bail") continue;
            if (!present) continue;
            if (rule == "current_password") {
                if (!current_password_matches(value.value_or(std::string_view{}), argument)) {
                    errors_.push_back({"Validation Failed", message_for(field, rule, argument), field});
                    field_valid = false;
                    if (std::find(field_rules.begin(), field_rules.end(), "bail") != field_rules.end()) break;
                }
                continue;
            }
            if (is_null(field)) {
                if (nullable) continue;
                errors_.push_back({"Validation Failed", message_for(field, rule, argument), field});
                field_valid = false;
                if (std::find(field_rules.begin(), field_rules.end(), "bail") != field_rules.end()) break;
                continue;
            }
            if (!accepts(*this, field, value.value_or(std::string_view{}), rule, argument,
                         numeric_attribute)) {
                errors_.push_back({"Validation Failed", message_for(field, rule, argument), field});
                field_valid = false;
                if (std::find(field_rules.begin(), field_rules.end(), "bail") != field_rules.end()) {
                    stop_field = true;
                    break;
                }
            }
        }
        (void)stop_field;
        if (field_valid && present) {
            if (value && !composite) validated_.insert_or_assign(field, std::string(*value));
            else if (const auto* uploaded = file(field))
                validated_.insert_or_assign(field, uploaded->name);
            else if (composite) {
                const auto flattened = all();
                const std::string prefix = field + ".";
                for (const auto& [key, item] : flattened)
                    if (key.starts_with(prefix)) validated_.insert_or_assign(key, item);
            }
        }
    }
    return errors_.empty();
}

const std::map<std::string, std::string>& FormRequest::validated() const {
    if (!validation_ran_) throw std::logic_error("Call FormRequest::validate() before validated()");
    if (!authorized_ || !errors_.empty())
        throw std::logic_error("Validated input is unavailable when the form request has errors");
    return validated_;
}

Response FormRequest::error_response() const {
    if (!validation_ran_) throw std::logic_error("Call FormRequest::validate() before error_response()");
    if (authorized_ && errors_.empty())
        throw std::logic_error("FormRequest has no validation or authorization error");
    if (kind() == RouteKind::api) {
        if (!authorized_)
            return ApiResponse::error(403, "Forbidden", "This action is not authorized.");
        if (!has_valid_input())
            return ApiResponse::error(400, "Bad Request", "The request body contains invalid JSON.");
        return ApiResponse::error(422, errors_);
    }

    const int status = !authorized_ ? 403 : (has_valid_input() ? 422 : 400);
    const std::string title = status == 403 ? "403 Forbidden" :
        status == 400 ? "400 Bad Request" : "422 Unprocessable Entity";
    std::string page = "<!doctype html><html><head><title>" + title +
        "</title></head><body><h1>" + title + "</h1><ul>";
    if (!authorized_) page += "<li>This action is not authorized.</li>";
    if (!has_valid_input()) page += "<li>The request body contains invalid JSON.</li>";
    for (const auto& error : errors_)
        page += "<li>" + escape_html(error.detail) + "</li>";
    page += "</ul></body></html>";
    return Response{status, "text/html; charset=utf-8", std::move(page)};
}

} // namespace kotavel::http
