#include <kotavel/session/session.hpp>

#include "session_internal.hpp"

#include <rapidjson/document.h>
#include <rapidjson/stringbuffer.h>
#include <rapidjson/writer.h>

#include <algorithm>
#include <limits>
#include <map>
#include <system_error>

namespace kotavel::http {
namespace {
using Writer = rapidjson::Writer<rapidjson::StringBuffer>;

bool write_json(Writer& writer, const JsonValue& value) {
    return std::visit([&](const auto& item) -> bool {
        using T = std::decay_t<decltype(item)>;
        if constexpr (std::is_same_v<T, std::nullptr_t>) return writer.Null();
        else if constexpr (std::is_same_v<T, bool>) return writer.Bool(item);
        else if constexpr (std::is_same_v<T, std::int64_t>) return writer.Int64(item);
        else if constexpr (std::is_same_v<T, std::uint64_t>) return writer.Uint64(item);
        else if constexpr (std::is_same_v<T, double>) return writer.Double(item);
        else if constexpr (std::is_same_v<T, std::string>)
            return writer.String(item.data(), static_cast<rapidjson::SizeType>(item.size()));
        else if constexpr (std::is_same_v<T, JsonValue::Array>) {
            if (!writer.StartArray()) return false;
            for (const auto& child : item) if (!write_json(writer, child)) return false;
            return writer.EndArray();
        } else {
            if (!writer.StartObject()) return false;
            for (const auto& [key, child] : item) {
                if (!writer.Key(key.data(), static_cast<rapidjson::SizeType>(key.size())) ||
                    !write_json(writer, child)) return false;
            }
            return writer.EndObject();
        }
    }, value.value());
}

JsonValue read_json(const rapidjson::Value& value) {
    if (value.IsNull()) return nullptr;
    if (value.IsBool()) return value.GetBool();
    if (value.IsInt64()) return value.GetInt64();
    if (value.IsUint64()) return value.GetUint64();
    if (value.IsNumber()) return value.GetDouble();
    if (value.IsString()) return std::string(value.GetString(), value.GetStringLength());
    if (value.IsArray()) {
        JsonValue::Array result;
        result.reserve(value.Size());
        for (const auto& child : value.GetArray()) result.push_back(read_json(child));
        return result;
    }
    JsonValue::Object result;
    for (auto member = value.MemberBegin(); member != value.MemberEnd(); ++member)
        result.emplace(std::string(member->name.GetString(), member->name.GetStringLength()),
                       read_json(member->value));
    return result;
}

bool read_timestamp(const rapidjson::Value& object, const char* key, std::int64_t& result) {
    const auto field = object.FindMember(key);
    if (field == object.MemberEnd()) return false;
    if (field->value.IsInt64()) result = field->value.GetInt64();
    else if (field->value.IsUint64() && field->value.GetUint64() <=
             static_cast<std::uint64_t>(std::numeric_limits<std::int64_t>::max()))
        result = static_cast<std::int64_t>(field->value.GetUint64());
    else return false;
    return result >= 0;
}

std::string serialize_record(const contracts::SessionRecord& record) {
    rapidjson::StringBuffer buffer;
    Writer writer(buffer);
    writer.StartObject();
    writer.Key("version"); writer.Int(1);
    writer.Key("id"); writer.String(record.id.data(), static_cast<rapidjson::SizeType>(record.id.size()));
    writer.Key("created_at"); writer.Int64(record.created_at);
    writer.Key("last_activity"); writer.Int64(record.last_activity);
    writer.Key("expires_at"); writer.Int64(record.expires_at);
    writer.Key("data");
    if (!write_json(writer, JsonValue(record.values)))
        throw std::runtime_error("Could not serialize session data");
    writer.EndObject();
    return {buffer.GetString(), buffer.GetSize()};
}

std::optional<contracts::SessionRecord> parse_record(std::string_view bytes,
                                                      std::string_view expected_id) {
    rapidjson::Document document;
    document.Parse(bytes.data(), bytes.size());
    if (document.HasParseError() || !document.IsObject()) return std::nullopt;
    const auto version = document.FindMember("version");
    const auto id = document.FindMember("id");
    const auto data = document.FindMember("data");
    if (version == document.MemberEnd() || !version->value.IsInt() || version->value.GetInt() != 1 ||
        id == document.MemberEnd() || !id->value.IsString() ||
        std::string_view(id->value.GetString(), id->value.GetStringLength()) != expected_id ||
        data == document.MemberEnd() || !data->value.IsObject()) return std::nullopt;

    contracts::SessionRecord result;
    result.id = expected_id;
    if (!read_timestamp(document, "created_at", result.created_at) ||
        !read_timestamp(document, "last_activity", result.last_activity) ||
        !read_timestamp(document, "expires_at", result.expires_at)) return std::nullopt;
    auto values = read_json(data->value);
    result.values = std::get<JsonValue::Object>(std::move(values.value()));
    return result;
}

class SessionLockAdapter final : public contracts::SessionStore::Lock {
public:
    explicit SessionLockAdapter(std::unique_ptr<contracts::FileLock> lock)
        : lock_(std::move(lock)) {}
private:
    std::unique_ptr<contracts::FileLock> lock_;
};
}

namespace detail {
bool valid_session_id(std::string_view id) noexcept {
    if (id.size() != 64) return false;
    for (const unsigned char ch : id) {
        if (!((ch >= '0' && ch <= '9') || (ch >= 'a' && ch <= 'f'))) return false;
    }
    return true;
}

std::string generate_session_id() {
    unsigned char bytes[32]{};
    fill_secure_random(bytes);
    constexpr char digits[] = "0123456789abcdef";
    std::string result;
    result.resize(sizeof(bytes) * 2);
    for (std::size_t index = 0; index < sizeof(bytes); ++index) {
        result[index * 2] = digits[bytes[index] >> 4];
        result[index * 2 + 1] = digits[bytes[index] & 0x0f];
    }
    return result;
}
} // namespace detail

FileSessionStore::FileSessionStore(
    std::filesystem::path directory,
    std::shared_ptr<contracts::Filesystem> file_system)
    : directory_(std::move(directory)), lock_directory_(directory_ / ".locks"),
      filesystem_(file_system ? std::move(file_system) :
          std::make_shared<filesystem::LocalFilesystem>()) {
    if (directory_.empty()) throw std::invalid_argument("Session directory cannot be empty");
    ensure_directories(true);
}

void FileSessionStore::ensure_directories(bool enforce_permissions) {
    filesystem_->create_directories(directory_, enforce_permissions);
    filesystem_->create_directories(lock_directory_, enforce_permissions);
}

std::unique_ptr<contracts::SessionStore::Lock> FileSessionStore::lock(std::string_view id) {
    if (!detail::valid_session_id(id)) throw std::invalid_argument("Invalid session identifier");
    ensure_directories();
    // A bounded shard set avoids leaving one permanent lock file per expired session.
    const auto path = lock_directory_ / (std::string(id.substr(0, 3)) + ".lock");
    try {
        return std::make_unique<SessionLockAdapter>(filesystem_->lock_file(path));
    } catch (const std::system_error& error) {
        // Recover if the session directory was removed between ensure and open.
        if (error.code() != std::errc::no_such_file_or_directory) throw;
        ensure_directories();
        return std::make_unique<SessionLockAdapter>(filesystem_->lock_file(path));
    }
}

std::filesystem::path FileSessionStore::record_path(std::string_view id) const {
    return directory_ / std::string(id.substr(0, 2)) / (std::string(id) + ".json");
}

std::optional<contracts::SessionRecord> FileSessionStore::read_record(
    const std::filesystem::path& path, std::string_view id, bool& present) {
    present = false;
    std::optional<std::string> bytes;
    try {
        bytes = filesystem_->read_file(path, 4 * 1024 * 1024);
    } catch (const std::length_error&) {
        present = true;
        (void)filesystem_->remove_file(path);
        return std::nullopt;
    }
    if (!bytes) return std::nullopt;
    present = true;
    auto record = parse_record(*bytes, id);
    if (!record) (void)filesystem_->remove_file(path);
    return record;
}

std::optional<contracts::SessionRecord> FileSessionStore::load(std::string_view id) {
    if (!detail::valid_session_id(id)) return std::nullopt;
    bool present = false;
    auto record = read_record(record_path(id), id, present);
    if (present) {
        // A corrupt new record must never resurrect an older legacy copy.
        if (!record) remove(id);
        return record;
    }
    const auto legacy = directory_ / (std::string(id) + ".json");
    record = read_record(legacy, id, present);
    if (record) {
        // The caller holds the unchanged ID lock. Commit the new location before
        // removing the old one; a failed write leaves the legacy session intact.
        save(*record);
        (void)filesystem_->remove_file(legacy);
    }
    return record;
}

void FileSessionStore::save(const contracts::SessionRecord& record) {
    if (!detail::valid_session_id(record.id)) throw std::invalid_argument("Invalid session identifier");
    if (record.created_at < 0 || record.last_activity < 0 || record.expires_at < 0)
        throw std::invalid_argument("Session timestamps cannot be negative");
    const auto destination = record_path(record.id);
    const auto bytes = serialize_record(record);
    if (bytes.size() > 4 * 1024 * 1024)
        throw std::length_error("Session data exceeds the 4 MiB storage limit");
    ensure_directories();
    filesystem_->create_directories(destination.parent_path());
    filesystem_->write_file_atomically(destination, bytes, true);
}

void FileSessionStore::remove(std::string_view id) {
    if (!detail::valid_session_id(id)) return;
    // Remove both layouts so an old duplicate cannot revive an invalidated ID.
    (void)filesystem_->remove_file(directory_ / (std::string(id) + ".json"));
    (void)filesystem_->remove_file(record_path(id));
}

void FileSessionStore::collect_expired(std::int64_t now) {
    const std::unique_lock guard(collection_mutex_, std::try_to_lock);
    if (!guard.owns_lock()) return;
    ensure_directories();
    constexpr std::size_t file_budget = 128;
    constexpr unsigned directory_budget = 257;
    constexpr char hex[] = "0123456789abcdef";
    std::map<std::string, std::vector<std::filesystem::path>, std::less<>> shards;
    std::size_t inspected = 0;
    unsigned completed_directories = 0;
    while (inspected < file_budget && completed_directories < directory_budget) {
        if (!collection_cursor_) {
            auto directory = directory_;
            if (collection_directory_ != 0) {
                const auto prefix = collection_directory_ - 1;
                directory /= std::string{hex[prefix / 16], hex[prefix % 16]};
            }
            try { collection_cursor_ = filesystem_->scan_files(directory); }
            catch (const std::system_error& error) {
                if (error.code() != std::errc::no_such_file_or_directory) throw;
            }
        }
        std::optional<std::filesystem::path> path;
        try { if (collection_cursor_) path = collection_cursor_->next(); }
        catch (const std::system_error& error) {
            collection_cursor_.reset();
            if (error.code() != std::errc::no_such_file_or_directory) throw;
        }
        if (!path) {
            collection_cursor_.reset();
            collection_directory_ = (collection_directory_ + 1) % 257;
            ++completed_directories;
            continue;
        }
        ++inspected;
        if (path->extension() != ".json") continue;
        const auto id = path->stem().string();
        if (!detail::valid_session_id(id)) continue;
        // Never act on a record misplaced in another ID's prefix directory.
        if (path->parent_path() != directory_ && *path != record_path(id)) continue;
        shards[id.substr(0, 3)].push_back(std::move(*path));
    }

    for (const auto& [shard, paths] : shards) {
        auto session_lock = lock(shard + std::string(61, '0'));
        std::vector<std::filesystem::path> expired;
        for (const auto& path : paths) {
            const auto id = path.stem().string();
            bool present = false;
            // Inspect the selected layout without migrating files during a scan.
            const auto record = read_record(path, id, present);
            if (!record) {
                if (present && path.parent_path() != directory_)
                    (void)filesystem_->remove_file(directory_ / (id + ".json"));
                continue;
            }
            if (record->expires_at > now) continue;
            if (path.parent_path() != directory_) {
                // The new copy is authoritative; remove any stale legacy copy
                // first so deletion cannot expose it on a later load.
                (void)filesystem_->remove_file(directory_ / (id + ".json"));
            }
            expired.push_back(path);
        }
        if (!expired.empty()) filesystem_->remove_files(expired);
    }
}

} // namespace kotavel::http
