#include <kotavel/session/session.hpp>
#include <kotavel/filesystem/filesystem.hpp>

#include "session_internal.hpp"
#include "session_size.hpp"

#include <rapidjson/document.h>
#include <rapidjson/stringbuffer.h>
#include <rapidjson/writer.h>
#include <sqlite3.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <condition_variable>
#include <deque>
#include <functional>
#include <future>
#include <limits>
#include <mutex>
#include <stdexcept>
#include <thread>
#include <type_traits>
#include <utility>
#include <chrono>

namespace kotavel::http {
namespace {
using Writer = rapidjson::Writer<rapidjson::StringBuffer>;
constexpr std::int64_t maximum_contexts = 4096;
constexpr std::int64_t maximum_context_grants = 32;
constexpr std::int64_t maximum_context_values = 1024;
constexpr std::int64_t maximum_context_bytes = 4 * 1024 * 1024;
constexpr std::size_t maximum_context_value_bytes = 256 * 1024;

template <typename T>
void update_max(std::atomic<T>& maximum, T value) noexcept {
    auto current = maximum.load(std::memory_order_relaxed);
    while (current < value &&
           !maximum.compare_exchange_weak(current, value, std::memory_order_relaxed)) {}
}

void sqlite_check(sqlite3* db, int result, std::string_view operation) {
    if (result != SQLITE_OK && result != SQLITE_DONE && result != SQLITE_ROW) {
        const char* detail = db ? sqlite3_errmsg(db) : sqlite3_errstr(result);
        throw std::runtime_error(std::string(operation) + ": " + detail);
    }
}

void execute(sqlite3* db, const char* sql) {
    char* message = nullptr;
    const int result = sqlite3_exec(db, sql, nullptr, nullptr, &message);
    if (result == SQLITE_OK) return;
    const std::string detail = message ? message : sqlite3_errmsg(db);
    sqlite3_free(message);
    throw std::runtime_error(std::string("SQLite session store: ") + detail);
}

class Statement final {
public:
    Statement(sqlite3* db, const char* sql) : db_(db) {
        const int result = sqlite3_prepare_v2(db_, sql, -1, &statement_, nullptr);
        if (result == SQLITE_OK) return;
        if (statement_) {
            sqlite3_finalize(statement_);
            statement_ = nullptr;
        }
        sqlite_check(db_, result, "Could not prepare SQLite session statement");
    }
    ~Statement() { sqlite3_finalize(statement_); }
    Statement(const Statement&) = delete;
    Statement& operator=(const Statement&) = delete;
    [[nodiscard]] sqlite3_stmt* get() const noexcept { return statement_; }
    [[nodiscard]] sqlite3_stmt* release() noexcept { return std::exchange(statement_, nullptr); }
private:
    sqlite3* db_;
    sqlite3_stmt* statement_ = nullptr;
};

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

std::string serialize_values(const JsonValue::Object& values) {
    rapidjson::StringBuffer buffer;
    Writer writer(buffer);
    if (!write_json(writer, JsonValue(values)))
        throw std::runtime_error("Could not serialize SQLite session values");
    if (buffer.GetSize() > 4 * 1024 * 1024)
        throw std::length_error("Session data exceeds the 4 MiB storage limit");
    return {buffer.GetString(), buffer.GetSize()};
}

JsonValue::Object parse_values(const unsigned char* bytes, int length) {
    rapidjson::Document document;
    document.Parse(reinterpret_cast<const char*>(bytes), static_cast<std::size_t>(length));
    if (document.HasParseError() || !document.IsObject())
        throw std::runtime_error("SQLite session record contains invalid JSON data");
    auto parsed = read_json(document);
    return std::get<JsonValue::Object>(std::move(parsed.value()));
}

void bind_text(sqlite3* db, sqlite3_stmt* statement, int index, std::string_view value) {
    sqlite_check(db, sqlite3_bind_text(statement, index, value.data(),
        static_cast<int>(value.size()), SQLITE_TRANSIENT), "Could not bind SQLite session value");
}

std::string path_utf8(const std::filesystem::path& path) {
    const auto encoded = path.u8string();
    return {reinterpret_cast<const char*>(encoded.data()), encoded.size()};
}

std::int64_t epoch_seconds() {
    return std::chrono::duration_cast<std::chrono::seconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
}

std::string serialize_context_value(const JsonValue& value) {
    return serialize_values(JsonValue::Object{{"value", value}});
}

JsonValue parse_context_value(const unsigned char* data, int size) {
    const auto values = parse_values(data, size);
    const auto found = values.find("value");
    if (found == values.end()) throw std::runtime_error("SQLite context value is malformed");
    return found->second;
}

class NoopSessionLock final : public contracts::SessionStore::Lock {};
} // namespace

struct SQLiteSessionStore::State final {
    struct Job final {
        std::function<void(sqlite3*)> run;
        std::function<bool()> failed;
        std::function<void()> complete;
        std::function<void(std::exception_ptr)> fail;
        std::chrono::steady_clock::time_point enqueued_at;
        bool batchable = false;
    };

    struct Reader final {
        sqlite3* db = nullptr;
        sqlite3_stmt* load = nullptr;
        bool in_use = false;
    };

    State(std::filesystem::path database_path, std::size_t read_connection_count,
          std::size_t write_queue_limit, int timeout_ms,
          std::chrono::microseconds write_batch_window_value)
        : path(std::move(database_path)), queue_limit(write_queue_limit),
          busy_timeout(timeout_ms), write_batch_window(write_batch_window_value) {
        if (path.empty()) throw std::invalid_argument("SQLite session database path cannot be empty");
        if (read_connection_count == 0)
            throw std::invalid_argument("SQLite session store requires at least one read connection");
        if (queue_limit == 0)
            throw std::invalid_argument("SQLite session write queue capacity must be greater than zero");
        if (busy_timeout < 0)
            throw std::invalid_argument("SQLite busy timeout cannot be negative");
        if (write_batch_window < std::chrono::microseconds::zero() ||
            write_batch_window > std::chrono::milliseconds(10))
            throw std::invalid_argument("SQLite session write batch window must be between 0 and 10000 microseconds");

        try {
            if (!path.parent_path().empty())
                filesystem::LocalFilesystem{}.create_directories(path.parent_path(), true);
            const auto encoded_path = path_utf8(path);
            sqlite_check(nullptr, sqlite3_open_v2(encoded_path.c_str(), &writer,
                SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX, nullptr),
                "Could not open SQLite session database");
            sqlite3_busy_timeout(writer, busy_timeout);
            std::error_code permission_error;
            std::filesystem::permissions(path,
                std::filesystem::perms::owner_read | std::filesystem::perms::owner_write,
                std::filesystem::perm_options::replace, permission_error);
            if (permission_error)
                throw std::filesystem::filesystem_error(
                    "Could not restrict SQLite session database permissions", path, permission_error);
            execute(writer, "PRAGMA journal_mode=WAL");
            execute(writer, "PRAGMA synchronous=FULL");
            execute(writer, "PRAGMA foreign_keys=ON");
            execute(writer, "CREATE TABLE IF NOT EXISTS sessions ("
                "id TEXT PRIMARY KEY NOT NULL, created_at INTEGER NOT NULL, "
                "last_activity INTEGER NOT NULL, expires_at INTEGER NOT NULL, "
                "revision INTEGER NOT NULL, data TEXT NOT NULL)");
            execute(writer, "CREATE TABLE IF NOT EXISTS kotavel_session_meta ("
                "key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL)");
            execute(writer, "CREATE TABLE IF NOT EXISTS session_contexts ("
                "context_id TEXT PRIMARY KEY NOT NULL, created_at INTEGER NOT NULL)");
            execute(writer, "CREATE TABLE IF NOT EXISTS session_context_grants ("
                "grant_token TEXT PRIMARY KEY NOT NULL, context_id TEXT NOT NULL, "
                "expires_at INTEGER NOT NULL, FOREIGN KEY(context_id) "
                "REFERENCES session_contexts(context_id) ON DELETE CASCADE)");
            execute(writer, "CREATE TABLE IF NOT EXISTS session_context_links ("
                "session_id TEXT PRIMARY KEY NOT NULL, context_id TEXT NOT NULL, "
                "linked_at INTEGER NOT NULL, FOREIGN KEY(context_id) "
                "REFERENCES session_contexts(context_id) ON DELETE CASCADE)");
            execute(writer, "CREATE TABLE IF NOT EXISTS session_context_values ("
                "context_id TEXT NOT NULL, key TEXT NOT NULL, data TEXT NOT NULL, "
                "expires_at INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(context_id, key), "
                "FOREIGN KEY(context_id) REFERENCES session_contexts(context_id) ON DELETE CASCADE)");
            execute(writer, "CREATE INDEX IF NOT EXISTS sessions_expiry_idx "
                            "ON sessions(expires_at, id)");
            execute(writer, "CREATE INDEX IF NOT EXISTS session_context_grants_expiry_idx "
                            "ON session_context_grants(expires_at, context_id)");
            execute(writer, "CREATE INDEX IF NOT EXISTS session_context_grants_context_expiry_idx "
                            "ON session_context_grants(context_id, expires_at)");
            execute(writer, "CREATE INDEX IF NOT EXISTS session_context_values_expiry_idx "
                            "ON session_context_values(expires_at, context_id)");

            const auto generated_instance_id = detail::generate_session_id();
            {
                Statement insert(writer, "INSERT OR IGNORE INTO kotavel_session_meta(key, value) "
                    "VALUES('instance_id', ?1)");
                bind_text(writer, insert.get(), 1, generated_instance_id);
                sqlite_check(writer, sqlite3_step(insert.get()), "Could not initialize session instance ID");
                Statement select(writer, "SELECT value FROM kotavel_session_meta WHERE key='instance_id'");
                const int result = sqlite3_step(select.get());
                sqlite_check(writer, result, "Could not load session instance ID");
                instance_identifier = reinterpret_cast<const char*>(sqlite3_column_text(select.get(), 0));
            }

            readers.reserve(read_connection_count);
            for (std::size_t index = 0; index < read_connection_count; ++index) {
                readers.push_back(std::make_unique<Reader>());
                auto& reader = *readers.back();
                const int open_result = sqlite3_open_v2(encoded_path.c_str(), &reader.db,
                    SQLITE_OPEN_READONLY | SQLITE_OPEN_FULLMUTEX, nullptr);
                sqlite_check(reader.db, open_result, "Could not open SQLite session read connection");
                sqlite3_busy_timeout(reader.db, busy_timeout);
                Statement statement(reader.db, "SELECT id, created_at, last_activity, "
                    "expires_at, revision, data FROM sessions WHERE id = ?1");
                reader.load = statement.get();
                // Ownership is transferred from the temporary Statement.
                (void)statement.release();
            }
            writer_thread = std::thread([this] { writer_loop(); });
        } catch (...) {
            close();
            throw;
        }
    }

    ~State() {
        {
            std::lock_guard guard(queue_mutex);
            stopping = true;
        }
        queue_ready.notify_all();
        queue_capacity.notify_all();
        if (writer_thread.joinable()) writer_thread.join();
        close();
    }

    void close() noexcept {
        for (auto& reader : readers) {
            if (reader->load) sqlite3_finalize(reader->load);
            if (reader->db) sqlite3_close_v2(reader->db);
        }
        readers.clear();
        if (writer) sqlite3_close_v2(writer);
        writer = nullptr;
    }

    void writer_loop() {
        for (;;) {
            std::vector<Job> jobs;
            {
                std::unique_lock guard(queue_mutex);
                queue_ready.wait(guard, [this] { return stopping || !queue.empty(); });
                if (queue.empty() && stopping) return;
                jobs.push_back(std::move(queue.front()));
                queue.pop_front();
                if (jobs.front().batchable) {
                    const auto deadline = std::chrono::steady_clock::now() + write_batch_window;
                    while (jobs.size() < 64) {
                        while (!queue.empty() && queue.front().batchable && jobs.size() < 64) {
                            jobs.push_back(std::move(queue.front()));
                            queue.pop_front();
                        }
                        if (jobs.size() == 64 || (!queue.empty() && !queue.front().batchable) ||
                            write_batch_window == std::chrono::microseconds::zero() || stopping)
                            break;
                        if (queue_ready.wait_until(guard, deadline) == std::cv_status::timeout)
                            break;
                    }
                }
            }
            queue_capacity.notify_all();
            if (jobs.front().batchable) execute_batch(jobs);
            else execute_one(jobs.front());
        }
    }

    void record_job_started(const Job& job) {
        const auto started_at = std::chrono::steady_clock::now();
        const auto queue_ns = static_cast<std::uint64_t>(
            std::chrono::duration_cast<std::chrono::nanoseconds>(
                started_at - job.enqueued_at).count());
        queue_wait_total_ns.fetch_add(queue_ns, std::memory_order_relaxed);
        update_max(queue_wait_max_ns, queue_ns);
        write_jobs.fetch_add(1, std::memory_order_relaxed);
    }

    void record_job_finished(std::chrono::steady_clock::time_point started_at) {
        const auto service_ns = static_cast<std::uint64_t>(
            std::chrono::duration_cast<std::chrono::nanoseconds>(
                std::chrono::steady_clock::now() - started_at).count());
        service_total_ns.fetch_add(service_ns, std::memory_order_relaxed);
        update_max(service_max_ns, service_ns);
    }

    void execute_one(Job& job) {
        const auto started_at = std::chrono::steady_clock::now();
        record_job_started(job);
        job.run(writer);
        record_job_finished(started_at);
    }

    void execute_batch(std::vector<Job>& jobs) {
        const auto batch_started_at = std::chrono::steady_clock::now();
        try {
            execute(writer, "BEGIN IMMEDIATE");
            for (auto& job : jobs) {
                execute(writer, "SAVEPOINT kotavel_session_commit");
                record_job_started(job);
                job.run(writer);
                if (job.failed()) {
                    execute(writer, "ROLLBACK TO kotavel_session_commit");
                }
                execute(writer, "RELEASE kotavel_session_commit");
            }
            execute(writer, "COMMIT");
            const auto batch_service_ns = static_cast<std::uint64_t>(
                std::chrono::duration_cast<std::chrono::nanoseconds>(
                    std::chrono::steady_clock::now() - batch_started_at).count());
            service_total_ns.fetch_add(batch_service_ns, std::memory_order_relaxed);
            update_max(service_max_ns, batch_service_ns / jobs.size());
            write_batches.fetch_add(1, std::memory_order_relaxed);
            batched_write_jobs.fetch_add(jobs.size(), std::memory_order_relaxed);
            update_max(max_batch_size, jobs.size());
        } catch (...) {
            const auto error = std::current_exception();
            sqlite3_exec(writer, "ROLLBACK", nullptr, nullptr, nullptr);
            for (auto& job : jobs) job.fail(error);
            return;
        }
        for (auto& job : jobs) job.complete();
    }

    void enqueue(Job job) {
        const auto admission_started = std::chrono::steady_clock::now();
        {
            std::unique_lock guard(queue_mutex);
            queue_capacity.wait(guard, [this] { return stopping || queue.size() < queue_limit; });
            if (stopping) throw std::runtime_error("SQLite session store is shutting down");
            job.enqueued_at = std::chrono::steady_clock::now();
            const auto admission_ns = static_cast<std::uint64_t>(
                std::chrono::duration_cast<std::chrono::nanoseconds>(
                    job.enqueued_at - admission_started).count());
            admission_wait_total_ns.fetch_add(admission_ns, std::memory_order_relaxed);
            queue.emplace_back(std::move(job));
            update_max(peak_queue_depth, queue.size());
        }
        queue_ready.notify_one();
    }

    template <typename Function>
    auto write(Function&& function) -> std::invoke_result_t<Function, sqlite3*> {
        using Result = std::invoke_result_t<Function, sqlite3*>;
        auto task = std::make_shared<std::packaged_task<Result(sqlite3*)>>(
            std::forward<Function>(function));
        auto future = task->get_future();
        Job job;
        job.run = [task](sqlite3* db) { (*task)(db); };
        enqueue(std::move(job));
        if constexpr (std::is_void_v<Result>) future.get();
        else return future.get();
    }

    template <typename Function>
    auto write_batched(Function&& function) -> std::invoke_result_t<Function, sqlite3*> {
        using Result = std::invoke_result_t<Function, sqlite3*>;
        static_assert(!std::is_void_v<Result>, "Batched writes must return a result");
        struct ResultState final {
            std::promise<Result> promise;
            std::optional<Result> value;
            std::exception_ptr error;
        };
        auto result = std::make_shared<ResultState>();
        auto future = result->promise.get_future();
        auto operation = std::make_shared<std::decay_t<Function>>(std::forward<Function>(function));
        Job job;
        job.batchable = true;
        job.run = [result, operation](sqlite3* db) {
            try { result->value.emplace((*operation)(db)); }
            catch (...) { result->error = std::current_exception(); }
        };
        job.failed = [result] { return static_cast<bool>(result->error); };
        job.complete = [result] {
            if (result->error) result->promise.set_exception(result->error);
            else result->promise.set_value(std::move(*result->value));
        };
        job.fail = [result](std::exception_ptr error) {
            result->promise.set_exception(std::move(error));
        };
        enqueue(std::move(job));
        return future.get();
    }

    Reader* acquire_reader() {
        std::unique_lock guard(reader_mutex);
        reader_available.wait(guard, [this] {
            return std::any_of(readers.begin(), readers.end(),
                [](const auto& reader) { return !reader->in_use; });
        });
        const auto found = std::find_if(readers.begin(), readers.end(),
            [](const auto& reader) { return !reader->in_use; });
        (*found)->in_use = true;
        return found->get();
    }

    void release_reader(Reader* reader) {
        {
            std::lock_guard guard(reader_mutex);
            reader->in_use = false;
        }
        reader_available.notify_one();
    }

    std::filesystem::path path;
    std::string instance_identifier;
    std::size_t queue_limit;
    int busy_timeout;
    std::chrono::microseconds write_batch_window;
    sqlite3* writer = nullptr;
    std::vector<std::unique_ptr<Reader>> readers;
    std::mutex reader_mutex;
    std::condition_variable reader_available;
    std::mutex queue_mutex;
    std::condition_variable queue_ready;
    std::condition_variable queue_capacity;
    std::deque<Job> queue;
    std::atomic<std::uint64_t> write_jobs{0};
    std::atomic<std::uint64_t> admission_wait_total_ns{0};
    std::atomic<std::uint64_t> queue_wait_total_ns{0};
    std::atomic<std::uint64_t> queue_wait_max_ns{0};
    std::atomic<std::uint64_t> service_total_ns{0};
    std::atomic<std::uint64_t> service_max_ns{0};
    std::atomic<std::size_t> peak_queue_depth{0};
    std::atomic<std::uint64_t> write_batches{0};
    std::atomic<std::uint64_t> batched_write_jobs{0};
    std::atomic<std::size_t> max_batch_size{0};
    bool stopping = false;
    std::thread writer_thread;
};

SQLiteSessionStore::SQLiteSessionStore(std::filesystem::path database,
                                       std::size_t read_connections,
                                       std::size_t max_queued_writes,
                                       int busy_timeout_ms,
                                       std::chrono::microseconds write_batch_window)
    : state_(std::make_unique<State>(std::move(database), read_connections,
                                    max_queued_writes, busy_timeout_ms,
                                    write_batch_window)) {}

SQLiteSessionStore::~SQLiteSessionStore() = default;

SQLiteSessionStore::Statistics SQLiteSessionStore::statistics() const {
    return {
        state_->write_jobs.load(std::memory_order_relaxed),
        state_->admission_wait_total_ns.load(std::memory_order_relaxed),
        state_->queue_wait_total_ns.load(std::memory_order_relaxed),
        state_->queue_wait_max_ns.load(std::memory_order_relaxed),
        state_->service_total_ns.load(std::memory_order_relaxed),
        state_->service_max_ns.load(std::memory_order_relaxed),
        state_->peak_queue_depth.load(std::memory_order_relaxed),
        state_->write_batches.load(std::memory_order_relaxed),
        state_->batched_write_jobs.load(std::memory_order_relaxed),
        state_->max_batch_size.load(std::memory_order_relaxed),
    };
}

std::unique_ptr<contracts::SessionStore::Lock> SQLiteSessionStore::lock(std::string_view) {
    return std::make_unique<NoopSessionLock>();
}

std::optional<contracts::SessionRecord> SQLiteSessionStore::load(std::string_view id) {
    if (!detail::valid_session_id(id)) return std::nullopt;
    auto* reader = state_->acquire_reader();
    struct Release final {
        State* state;
        State::Reader* reader;
        ~Release() { state->release_reader(reader); }
    } release{state_.get(), reader};

    sqlite3_reset(reader->load);
    sqlite3_clear_bindings(reader->load);
    bind_text(reader->db, reader->load, 1, id);
    const int result = sqlite3_step(reader->load);
    if (result == SQLITE_DONE) return std::nullopt;
    sqlite_check(reader->db, result, "Could not load SQLite session");

    contracts::SessionRecord record;
    record.id = reinterpret_cast<const char*>(sqlite3_column_text(reader->load, 0));
    record.created_at = sqlite3_column_int64(reader->load, 1);
    record.last_activity = sqlite3_column_int64(reader->load, 2);
    record.expires_at = sqlite3_column_int64(reader->load, 3);
    const auto revision = sqlite3_column_int64(reader->load, 4);
    if (revision < 1) throw std::runtime_error("SQLite session has an invalid revision");
    record.revision = static_cast<std::uint64_t>(revision);
    const auto* data = sqlite3_column_text(reader->load, 5);
    const int data_size = sqlite3_column_bytes(reader->load, 5);
    record.values = parse_values(data, data_size);
    record.estimated_values_bytes = detail::estimate_session_values_size(record.values);
    return record;
}

void SQLiteSessionStore::save(const contracts::SessionRecord& record) {
    if (!detail::valid_session_id(record.id)) throw std::invalid_argument("Invalid session identifier");
    if (record.created_at < 0 || record.last_activity < 0 || record.expires_at < 0 ||
        record.revision == 0 || record.revision > static_cast<std::uint64_t>(
            std::numeric_limits<sqlite3_int64>::max()))
        throw std::invalid_argument("SQLite session metadata is invalid");
    const auto data = serialize_values(record.values);
    state_->write([record, data](sqlite3* db) {
        Statement statement(db, "INSERT INTO sessions(id, created_at, last_activity, "
            "expires_at, revision, data) VALUES(?1, ?2, ?3, ?4, ?5, ?6) "
            "ON CONFLICT(id) DO UPDATE SET created_at=excluded.created_at, "
            "last_activity=excluded.last_activity, expires_at=excluded.expires_at, "
            "revision=excluded.revision, data=excluded.data");
        auto* query = statement.get();
        bind_text(db, query, 1, record.id);
        sqlite_check(db, sqlite3_bind_int64(query, 2, record.created_at), "Could not bind created_at");
        sqlite_check(db, sqlite3_bind_int64(query, 3, record.last_activity), "Could not bind last_activity");
        sqlite_check(db, sqlite3_bind_int64(query, 4, record.expires_at), "Could not bind expires_at");
        sqlite_check(db, sqlite3_bind_int64(query, 5, static_cast<sqlite3_int64>(record.revision)),
                     "Could not bind revision");
        bind_text(db, query, 6, data);
        sqlite_check(db, sqlite3_step(query), "Could not save SQLite session");
    });
}

void SQLiteSessionStore::remove(std::string_view id) {
    if (!detail::valid_session_id(id)) return;
    const std::string owned_id(id);
    state_->write([owned_id](sqlite3* db) {
        execute(db, "BEGIN IMMEDIATE");
        try {
            Statement links(db, "DELETE FROM session_context_links WHERE session_id=?1");
            bind_text(db, links.get(), 1, owned_id);
            sqlite_check(db, sqlite3_step(links.get()), "Could not remove session context link");
            Statement session(db, "DELETE FROM sessions WHERE id=?1");
            bind_text(db, session.get(), 1, owned_id);
            sqlite_check(db, sqlite3_step(session.get()), "Could not remove SQLite session");
            execute(db, "COMMIT");
        } catch (...) {
            sqlite3_exec(db, "ROLLBACK", nullptr, nullptr, nullptr);
            throw;
        }
    });
}

bool SQLiteSessionStore::commit(contracts::SessionRecord& record) {
    if (!detail::valid_session_id(record.id)) throw std::invalid_argument("Invalid session identifier");
    if (record.created_at < 0 || record.last_activity < 0 || record.expires_at < 0)
        throw std::invalid_argument("SQLite session timestamps are invalid");
    if (record.revision >= static_cast<std::uint64_t>(
            std::numeric_limits<sqlite3_int64>::max()))
        throw std::overflow_error("Session revision exhausted");
    const auto data = serialize_values(record.values);
    const auto expected = record.revision;
    const auto next = expected + 1;
    const bool committed = state_->write_batched([record, data, expected, next](sqlite3* db) {
        if (expected == 0) {
                Statement statement(db, "INSERT OR IGNORE INTO sessions(id, created_at, "
                    "last_activity, expires_at, revision, data) VALUES(?1, ?2, ?3, ?4, ?5, ?6)");
                auto* query = statement.get();
                bind_text(db, query, 1, record.id);
                sqlite_check(db, sqlite3_bind_int64(query, 2, record.created_at), "Could not bind created_at");
                sqlite_check(db, sqlite3_bind_int64(query, 3, record.last_activity), "Could not bind last_activity");
                sqlite_check(db, sqlite3_bind_int64(query, 4, record.expires_at), "Could not bind expires_at");
                sqlite_check(db, sqlite3_bind_int64(query, 5, static_cast<sqlite3_int64>(next)),
                             "Could not bind revision");
                bind_text(db, query, 6, data);
                sqlite_check(db, sqlite3_step(query), "Could not insert SQLite session");
        } else {
                Statement statement(db, "UPDATE sessions SET created_at=?1, last_activity=?2, "
                    "expires_at=?3, revision=?4, data=?5 WHERE id=?6 AND revision=?7");
                auto* query = statement.get();
                sqlite_check(db, sqlite3_bind_int64(query, 1, record.created_at), "Could not bind created_at");
                sqlite_check(db, sqlite3_bind_int64(query, 2, record.last_activity), "Could not bind last_activity");
                sqlite_check(db, sqlite3_bind_int64(query, 3, record.expires_at), "Could not bind expires_at");
                sqlite_check(db, sqlite3_bind_int64(query, 4, static_cast<sqlite3_int64>(next)),
                             "Could not bind revision");
                bind_text(db, query, 5, data);
                bind_text(db, query, 6, record.id);
                sqlite_check(db, sqlite3_bind_int64(query, 7, static_cast<sqlite3_int64>(expected)),
                             "Could not bind expected revision");
                sqlite_check(db, sqlite3_step(query), "Could not update SQLite session");
        }
        return sqlite3_changes(db) == 1;
    });
    if (committed) record.revision = next;
    return committed;
}

bool SQLiteSessionStore::commit_rotation(
    contracts::SessionRecord& replacement, std::uint64_t expected_previous_revision,
    std::span<const std::string> retired_ids) {
    if (!detail::valid_session_id(replacement.id))
        throw std::invalid_argument("Invalid replacement session identifier");
    if (retired_ids.empty()) return false;
    if (replacement.created_at < 0 || replacement.last_activity < 0 ||
        replacement.expires_at < 0)
        throw std::invalid_argument("SQLite session timestamps are invalid");
    if (expected_previous_revision >= static_cast<std::uint64_t>(
            std::numeric_limits<sqlite3_int64>::max()))
        throw std::overflow_error("Session revision exhausted");
    std::vector<std::string> retired(retired_ids.begin(), retired_ids.end());
    for (const auto& id : retired) {
        if (!detail::valid_session_id(id) || id == replacement.id)
            throw std::invalid_argument("Invalid retired session identifier");
    }
    const auto data = serialize_values(replacement.values);
    const auto now = epoch_seconds();
    const bool committed = state_->write_batched(
        [replacement, retired = std::move(retired), expected_previous_revision, data, now]
        (sqlite3* db) {
            for (std::size_t index = 0; index < retired.size(); ++index) {
                Statement previous(db, "SELECT revision, expires_at FROM sessions WHERE id=?1");
                bind_text(db, previous.get(), 1, retired[index]);
                const int result = sqlite3_step(previous.get());
                sqlite_check(db, result, "Could not validate the previous session before rotation");
                if (index == 0 && expected_previous_revision == 0) {
                    if (result == SQLITE_ROW) return false;
                } else if (index == 0) {
                    if (result != SQLITE_ROW || sqlite3_column_int64(previous.get(), 0) < 0 ||
                        static_cast<std::uint64_t>(sqlite3_column_int64(previous.get(), 0)) !=
                            expected_previous_revision ||
                        sqlite3_column_int64(previous.get(), 1) <= now) return false;
                } else if (result == SQLITE_ROW) {
                    // Intermediate IDs generated by repeated rotation in one request
                    // must never already refer to a durable identity.
                    return false;
                }
            }

            Statement replacement_exists(db, "SELECT 1 FROM sessions WHERE id=?1");
            bind_text(db, replacement_exists.get(), 1, replacement.id);
            const int replacement_result = sqlite3_step(replacement_exists.get());
            sqlite_check(db, replacement_result, "Could not validate replacement session identifier");
            if (replacement_result == SQLITE_ROW) return false;

            Statement insert(db, "INSERT INTO sessions(id, created_at, last_activity, expires_at, "
                                 "revision, data) VALUES(?1, ?2, ?3, ?4, 1, ?5)");
            auto* query = insert.get();
            bind_text(db, query, 1, replacement.id);
            sqlite_check(db, sqlite3_bind_int64(query, 2, replacement.created_at),
                         "Could not bind replacement created_at");
            sqlite_check(db, sqlite3_bind_int64(query, 3, replacement.last_activity),
                         "Could not bind replacement last_activity");
            sqlite_check(db, sqlite3_bind_int64(query, 4, replacement.expires_at),
                         "Could not bind replacement expires_at");
            bind_text(db, query, 5, data);
            sqlite_check(db, sqlite3_step(query), "Could not insert replacement session");

            for (const auto& id : retired) {
                Statement unlink(db, "DELETE FROM session_context_links WHERE session_id=?1");
                bind_text(db, unlink.get(), 1, id);
                sqlite_check(db, sqlite3_step(unlink.get()), "Could not revoke retired session context link");
                Statement remove(db, "DELETE FROM sessions WHERE id=?1");
                bind_text(db, remove.get(), 1, id);
                sqlite_check(db, sqlite3_step(remove.get()), "Could not remove retired session");
                if (id == retired.front() && expected_previous_revision != 0 &&
                    sqlite3_changes(db) != 1)
                    throw std::runtime_error("Previous session disappeared during atomic rotation");
            }
            return true;
        });
    if (committed) replacement.revision = 1;
    return committed;
}

bool SQLiteSessionStore::commit_rebased(
    contracts::SessionRecord& record,
    const std::function<bool(contracts::SessionRecord&)>& rebase) {
    if (!detail::valid_session_id(record.id))
        throw std::invalid_argument("Invalid session identifier");
    if (record.created_at < 0 || record.last_activity < 0 || record.expires_at < 0)
        throw std::invalid_argument("SQLite session timestamps are invalid");

    const auto session_id = record.id;
    return state_->write_batched([&record, rebase, session_id](sqlite3* db) {
        contracts::SessionRecord latest;
        {
            Statement select(db, "SELECT id, created_at, last_activity, expires_at, revision, data "
                                 "FROM sessions WHERE id=?1");
            bind_text(db, select.get(), 1, session_id);
            const int result = sqlite3_step(select.get());
            if (result == SQLITE_DONE) return false;
            sqlite_check(db, result, "Could not load session for atomic rebase");
            latest.id = reinterpret_cast<const char*>(sqlite3_column_text(select.get(), 0));
            latest.created_at = sqlite3_column_int64(select.get(), 1);
            latest.last_activity = sqlite3_column_int64(select.get(), 2);
            latest.expires_at = sqlite3_column_int64(select.get(), 3);
            const auto revision = sqlite3_column_int64(select.get(), 4);
            if (revision < 1) throw std::runtime_error("SQLite session has an invalid revision");
            latest.revision = static_cast<std::uint64_t>(revision);
            latest.values = parse_values(sqlite3_column_text(select.get(), 5),
                                          sqlite3_column_bytes(select.get(), 5));
            latest.estimated_values_bytes = detail::estimate_session_values_size(latest.values);
        }

        if (!rebase(latest)) return false;
        if (latest.id != session_id)
            throw std::logic_error("Session rebase callback changed the session identifier");
        if (latest.revision >= static_cast<std::uint64_t>(
                std::numeric_limits<sqlite3_int64>::max()))
            throw std::overflow_error("Session revision exhausted");
        ++latest.revision;
        const auto data = serialize_values(latest.values);
        Statement update(db, "UPDATE sessions SET created_at=?1, last_activity=?2, expires_at=?3, "
                             "revision=?4, data=?5 WHERE id=?6 AND revision=?7");
        auto* query = update.get();
        sqlite_check(db, sqlite3_bind_int64(query, 1, latest.created_at), "Could not bind created_at");
        sqlite_check(db, sqlite3_bind_int64(query, 2, latest.last_activity), "Could not bind last_activity");
        sqlite_check(db, sqlite3_bind_int64(query, 3, latest.expires_at), "Could not bind expires_at");
        sqlite_check(db, sqlite3_bind_int64(query, 4, static_cast<sqlite3_int64>(latest.revision)),
                     "Could not bind revision");
        bind_text(db, query, 5, data);
        bind_text(db, query, 6, session_id);
        sqlite_check(db, sqlite3_bind_int64(query, 7,
                         static_cast<sqlite3_int64>(latest.revision - 1)),
                     "Could not bind expected revision");
        sqlite_check(db, sqlite3_step(query), "Could not persist rebased SQLite session");
        if (sqlite3_changes(db) != 1) return false;
        record = std::move(latest);
        return true;
    });
}

SessionInstanceId SQLiteSessionStore::instance_id() const {
    return {state_->instance_identifier};
}

SessionContextId SQLiteSessionStore::create_context() {
    const auto context_id = detail::generate_session_id();
    const auto created_at = epoch_seconds();
    state_->write([context_id, created_at](sqlite3* db) {
        execute(db, "BEGIN IMMEDIATE");
        try {
            Statement count(db, "SELECT count(*) FROM session_contexts");
            sqlite_check(db, sqlite3_step(count.get()), "Could not count session contexts");
            if (sqlite3_column_int64(count.get(), 0) >= maximum_contexts)
                throw std::length_error("SQLite session context capacity is exhausted");
            Statement statement(db, "INSERT INTO session_contexts(context_id, created_at) VALUES(?1, ?2)");
            bind_text(db, statement.get(), 1, context_id);
            sqlite_check(db, sqlite3_bind_int64(statement.get(), 2, created_at),
                         "Could not bind context creation time");
            sqlite_check(db, sqlite3_step(statement.get()), "Could not create session context");
            execute(db, "COMMIT");
        } catch (...) {
            sqlite3_exec(db, "ROLLBACK", nullptr, nullptr, nullptr);
            throw;
        }
    });
    return {context_id};
}

SessionContextId SQLiteSessionStore::default_context() {
    const auto generated_id = detail::generate_session_id();
    const auto created_at = epoch_seconds();
    return state_->write([generated_id, created_at](sqlite3* db) {
        execute(db, "BEGIN IMMEDIATE");
        try {
            std::string context_id;
            Statement lookup(db, "SELECT value FROM kotavel_session_meta WHERE key='default_context_id'");
            const int result = sqlite3_step(lookup.get());
            sqlite_check(db, result, "Could not load default context ID");
            if (result == SQLITE_ROW) {
                const auto* text = sqlite3_column_text(lookup.get(), 0);
                context_id.assign(reinterpret_cast<const char*>(text));
                Statement exists(db, "SELECT 1 FROM session_contexts WHERE context_id=?1");
                bind_text(db, exists.get(), 1, context_id);
                const int exists_result = sqlite3_step(exists.get());
                sqlite_check(db, exists_result, "Could not validate default context");
                if (exists_result == SQLITE_ROW) {
                    execute(db, "COMMIT");
                    return SessionContextId{std::move(context_id)};
                }
            }

            Statement count(db, "SELECT count(*) FROM session_contexts");
            sqlite_check(db, sqlite3_step(count.get()), "Could not count session contexts");
            if (sqlite3_column_int64(count.get(), 0) >= maximum_contexts)
                throw std::length_error("SQLite session context capacity is exhausted");

            context_id = generated_id;
            Statement create(db, "INSERT INTO session_contexts(context_id, created_at) VALUES(?1, ?2)");
            bind_text(db, create.get(), 1, context_id);
            sqlite_check(db, sqlite3_bind_int64(create.get(), 2, created_at),
                         "Could not bind default context creation time");
            sqlite_check(db, sqlite3_step(create.get()), "Could not create default session context");
            Statement persist(db, "INSERT INTO kotavel_session_meta(key, value) "
                "VALUES('default_context_id', ?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value");
            bind_text(db, persist.get(), 1, context_id);
            sqlite_check(db, sqlite3_step(persist.get()), "Could not persist default context ID");
            execute(db, "COMMIT");
            return SessionContextId{std::move(context_id)};
        } catch (...) {
            sqlite3_exec(db, "ROLLBACK", nullptr, nullptr, nullptr);
            throw;
        }
    });
}

bool SQLiteSessionStore::delete_context(const SessionContextId& context) {
    if (!detail::valid_session_id(context.value)) return false;
    const std::string context_id = context.value;
    return state_->write([context_id](sqlite3* db) {
        Statement statement(db, "DELETE FROM session_contexts WHERE context_id=?1");
        bind_text(db, statement.get(), 1, context_id);
        sqlite_check(db, sqlite3_step(statement.get()), "Could not revoke session context");
        return sqlite3_changes(db) == 1;
    });
}

SessionLinkGrant SQLiteSessionStore::issue_link_grant(
    const SessionContextId& context, std::chrono::seconds lifetime) {
    if (!detail::valid_session_id(context.value))
        throw std::invalid_argument("Invalid session context identifier");
    if (lifetime <= std::chrono::seconds::zero() || lifetime > std::chrono::minutes(10))
        throw std::invalid_argument("Session link grant lifetime must be between 1 second and 10 minutes");
    const auto token = detail::generate_session_id();
    const auto expires_at = epoch_seconds() + lifetime.count();
    state_->write([context, token, expires_at](sqlite3* db) {
        execute(db, "BEGIN IMMEDIATE");
        try {
            Statement expired(db, "DELETE FROM session_context_grants WHERE rowid IN "
                "(SELECT rowid FROM session_context_grants WHERE expires_at<=?1 "
                "ORDER BY expires_at LIMIT 256)");
            sqlite_check(db, sqlite3_bind_int64(expired.get(), 1, epoch_seconds()),
                         "Could not bind expired grant cleanup time");
            sqlite_check(db, sqlite3_step(expired.get()), "Could not clean expired grants");
            Statement exists(db, "SELECT 1 FROM session_contexts WHERE context_id=?1");
            bind_text(db, exists.get(), 1, context.value);
            const int found = sqlite3_step(exists.get());
            sqlite_check(db, found, "Could not validate session context");
            if (found != SQLITE_ROW)
                throw std::invalid_argument("Unknown session context");
            Statement count(db, "SELECT count(*) FROM session_context_grants "
                "WHERE context_id=?1 AND expires_at>?2");
            bind_text(db, count.get(), 1, context.value);
            sqlite_check(db, sqlite3_bind_int64(count.get(), 2, epoch_seconds()),
                         "Could not bind active grant time");
            sqlite_check(db, sqlite3_step(count.get()), "Could not count active grants");
            if (sqlite3_column_int64(count.get(), 0) >= maximum_context_grants)
                throw std::length_error("Session context has too many active link grants");
            Statement insert(db, "INSERT INTO session_context_grants(grant_token, context_id, expires_at) "
                "VALUES(?1, ?2, ?3)");
            bind_text(db, insert.get(), 1, token);
            bind_text(db, insert.get(), 2, context.value);
            sqlite_check(db, sqlite3_bind_int64(insert.get(), 3, expires_at),
                         "Could not bind session grant expiration");
            sqlite_check(db, sqlite3_step(insert.get()), "Could not issue session link grant");
            execute(db, "COMMIT");
        } catch (...) {
            sqlite3_exec(db, "ROLLBACK", nullptr, nullptr, nullptr);
            throw;
        }
    });
    return {token};
}

bool SQLiteSessionStore::redeem_link_grant(const SessionLinkGrant& grant,
                                           std::string_view session_id) {
    if (!detail::valid_session_id(grant.value) || !detail::valid_session_id(session_id)) return false;
    const std::string token = grant.value;
    const std::string owned_session_id(session_id);
    const auto now = epoch_seconds();
    return state_->write([token, owned_session_id, now](sqlite3* db) {
        execute(db, "BEGIN IMMEDIATE");
        try {
            Statement session(db, "SELECT 1 FROM sessions WHERE id=?1 AND expires_at>?2");
            bind_text(db, session.get(), 1, owned_session_id);
            sqlite_check(db, sqlite3_bind_int64(session.get(), 2, now), "Could not bind session time");
            const int session_result = sqlite3_step(session.get());
            sqlite_check(db, session_result, "Could not validate target session");
            if (session_result != SQLITE_ROW) {
                execute(db, "ROLLBACK");
                return false;
            }

            Statement lookup(db, "SELECT context_id FROM session_context_grants "
                "WHERE grant_token=?1 AND expires_at>?2");
            bind_text(db, lookup.get(), 1, token);
            sqlite_check(db, sqlite3_bind_int64(lookup.get(), 2, now), "Could not bind grant time");
            const int grant_result = sqlite3_step(lookup.get());
            sqlite_check(db, grant_result, "Could not validate session link grant");
            if (grant_result != SQLITE_ROW) {
                execute(db, "ROLLBACK");
                return false;
            }
            const auto* context_text = sqlite3_column_text(lookup.get(), 0);
            const std::string context_id(reinterpret_cast<const char*>(context_text));

            Statement link(db, "INSERT OR IGNORE INTO session_context_links(session_id, context_id, linked_at) "
                "VALUES(?1, ?2, ?3)");
            bind_text(db, link.get(), 1, owned_session_id);
            bind_text(db, link.get(), 2, context_id);
            sqlite_check(db, sqlite3_bind_int64(link.get(), 3, now), "Could not bind link time");
            sqlite_check(db, sqlite3_step(link.get()), "Could not link session context");
            if (sqlite3_changes(db) != 1) {
                execute(db, "ROLLBACK");
                return false;
            }

            Statement consume(db, "DELETE FROM session_context_grants WHERE grant_token=?1");
            bind_text(db, consume.get(), 1, token);
            sqlite_check(db, sqlite3_step(consume.get()), "Could not consume session link grant");
            execute(db, "COMMIT");
            return true;
        } catch (...) {
            sqlite3_exec(db, "ROLLBACK", nullptr, nullptr, nullptr);
            throw;
        }
    });
}

std::optional<SessionContextId> SQLiteSessionStore::context_for_session(
    std::string_view session_id) {
    if (!detail::valid_session_id(session_id)) return std::nullopt;
    auto* reader = state_->acquire_reader();
    struct Release final {
        State* state;
        State::Reader* reader;
        ~Release() { state->release_reader(reader); }
    } release{state_.get(), reader};
    Statement statement(reader->db, "SELECT l.context_id FROM session_context_links l "
        "JOIN sessions s ON s.id=l.session_id WHERE l.session_id=?1 AND s.expires_at>?2");
    bind_text(reader->db, statement.get(), 1, session_id);
    sqlite_check(reader->db, sqlite3_bind_int64(statement.get(), 2, epoch_seconds()),
                 "Could not bind context lookup time");
    const int result = sqlite3_step(statement.get());
    if (result == SQLITE_DONE) return std::nullopt;
    sqlite_check(reader->db, result, "Could not resolve session context");
    const auto* text = sqlite3_column_text(statement.get(), 0);
    return SessionContextId{reinterpret_cast<const char*>(text)};
}

bool SQLiteSessionStore::revoke_context_link(std::string_view session_id) {
    if (!detail::valid_session_id(session_id)) return false;
    const std::string owned_id(session_id);
    return state_->write([owned_id](sqlite3* db) {
        Statement statement(db, "DELETE FROM session_context_links WHERE session_id=?1");
        bind_text(db, statement.get(), 1, owned_id);
        sqlite_check(db, sqlite3_step(statement.get()), "Could not revoke session context link");
        return sqlite3_changes(db) == 1;
    });
}

void SQLiteSessionStore::put_context_value(
    const SessionContextId& context, std::string_view key, JsonValue value,
    std::optional<std::chrono::seconds> lifetime) {
    if (!detail::valid_session_id(context.value))
        throw std::invalid_argument("Invalid session context identifier");
    if (key.empty() || key.size() > 256)
        throw std::invalid_argument("Session context keys must contain between 1 and 256 bytes");
    if (lifetime && (*lifetime <= std::chrono::seconds::zero() ||
                     *lifetime > std::chrono::hours(24 * 30)))
        throw std::invalid_argument("Session context value lifetime must be between 1 second and 30 days");
    const std::string owned_key(key);
    const auto data = serialize_context_value(value);
    if (data.size() > maximum_context_value_bytes)
        throw std::length_error("Session context value exceeds the 256 KiB limit");
    const auto expires_at = lifetime ? epoch_seconds() + lifetime->count() : 0;
    state_->write([context, owned_key, data, expires_at](sqlite3* db) {
        execute(db, "BEGIN IMMEDIATE");
        try {
            Statement cleanup(db, "DELETE FROM session_context_values WHERE context_id=?1 "
                "AND expires_at>0 AND expires_at<=?2");
            bind_text(db, cleanup.get(), 1, context.value);
            sqlite_check(db, sqlite3_bind_int64(cleanup.get(), 2, epoch_seconds()),
                         "Could not bind context TTL cleanup time");
            sqlite_check(db, sqlite3_step(cleanup.get()), "Could not clean expired context values");

            Statement context_exists(db, "SELECT 1 FROM session_contexts WHERE context_id=?1");
            bind_text(db, context_exists.get(), 1, context.value);
            const int exists = sqlite3_step(context_exists.get());
            sqlite_check(db, exists, "Could not validate context value owner");
            if (exists != SQLITE_ROW) throw std::invalid_argument("Unknown session context");

            Statement budget(db, "SELECT count(*), coalesce(sum(length(data)), 0) FROM "
                "session_context_values WHERE context_id=?1 AND key<>?2");
            bind_text(db, budget.get(), 1, context.value);
            bind_text(db, budget.get(), 2, owned_key);
            sqlite_check(db, sqlite3_step(budget.get()), "Could not inspect context storage budget");
            if (sqlite3_column_int64(budget.get(), 0) >= maximum_context_values ||
                sqlite3_column_int64(budget.get(), 1) + static_cast<sqlite3_int64>(data.size()) >
                    maximum_context_bytes)
                throw std::length_error("Session context storage budget is exhausted");

            Statement statement(db, "INSERT INTO session_context_values(context_id, key, data, expires_at) "
                "VALUES(?1, ?2, ?3, ?4) ON CONFLICT(context_id, key) DO UPDATE "
                "SET data=excluded.data, expires_at=excluded.expires_at");
            bind_text(db, statement.get(), 1, context.value);
            bind_text(db, statement.get(), 2, owned_key);
            bind_text(db, statement.get(), 3, data);
            sqlite_check(db, sqlite3_bind_int64(statement.get(), 4, expires_at),
                         "Could not bind context value expiration");
            sqlite_check(db, sqlite3_step(statement.get()), "Could not save session context value");
            execute(db, "COMMIT");
        } catch (...) {
            sqlite3_exec(db, "ROLLBACK", nullptr, nullptr, nullptr);
            throw;
        }
    });
}

std::optional<JsonValue> SQLiteSessionStore::get_context_value(
    const SessionContextId& context, std::string_view key) {
    if (!detail::valid_session_id(context.value) || key.empty() || key.size() > 256)
        return std::nullopt;
    auto* reader = state_->acquire_reader();
    struct Release final {
        State* state;
        State::Reader* reader;
        ~Release() { state->release_reader(reader); }
    } release{state_.get(), reader};
    Statement statement(reader->db, "SELECT data FROM session_context_values "
        "WHERE context_id=?1 AND key=?2 AND (expires_at=0 OR expires_at>?3)");
    bind_text(reader->db, statement.get(), 1, context.value);
    bind_text(reader->db, statement.get(), 2, key);
    sqlite_check(reader->db, sqlite3_bind_int64(statement.get(), 3, epoch_seconds()),
                 "Could not bind context value lookup time");
    const int result = sqlite3_step(statement.get());
    if (result == SQLITE_DONE) return std::nullopt;
    sqlite_check(reader->db, result, "Could not load session context value");
    const auto* data = sqlite3_column_text(statement.get(), 0);
    const int size = sqlite3_column_bytes(statement.get(), 0);
    return parse_context_value(data, size);
}

std::optional<JsonValue> SQLiteSessionStore::get_context_value_for_session(
    std::string_view session_id, std::string_view key) {
    if (!detail::valid_session_id(session_id) || key.empty() || key.size() > 256)
        return std::nullopt;
    auto* reader = state_->acquire_reader();
    struct Release final {
        State* state;
        State::Reader* reader;
        ~Release() { state->release_reader(reader); }
    } release{state_.get(), reader};
    const auto now = epoch_seconds();
    Statement statement(reader->db,
        "SELECT v.data FROM session_context_links l "
        "JOIN sessions s ON s.id=l.session_id "
        "JOIN session_context_values v ON v.context_id=l.context_id "
        "WHERE l.session_id=?1 AND s.expires_at>?2 AND v.key=?3 "
        "AND (v.expires_at=0 OR v.expires_at>?2)");
    bind_text(reader->db, statement.get(), 1, session_id);
    sqlite_check(reader->db, sqlite3_bind_int64(statement.get(), 2, now),
                 "Could not bind authorized context lookup time");
    bind_text(reader->db, statement.get(), 3, key);
    const int result = sqlite3_step(statement.get());
    if (result == SQLITE_DONE) return std::nullopt;
    sqlite_check(reader->db, result, "Could not load authorized context value");
    const auto* data = sqlite3_column_text(statement.get(), 0);
    const int size = sqlite3_column_bytes(statement.get(), 0);
    return parse_context_value(data, size);
}

SessionContextResolver::SessionContextResolver(std::shared_ptr<SQLiteSessionStore> store)
    : store_(std::move(store)) {
    if (!store_) throw std::invalid_argument("Session context resolver requires a SQLite store");
}

SessionInstanceId SessionContextResolver::instance_id() const { return store_->instance_id(); }
SessionContextId SessionContextResolver::create_context() { return store_->create_context(); }
SessionContextId SessionContextResolver::default_context() { return store_->default_context(); }
bool SessionContextResolver::delete_context(const SessionContextId& context) {
    return store_->delete_context(context);
}
SessionLinkGrant SessionContextResolver::issue_link_grant(
    const SessionContextId& context, std::chrono::seconds lifetime) {
    return store_->issue_link_grant(context, lifetime);
}
bool SessionContextResolver::redeem_link_grant(
    const SessionLinkGrant& grant, std::string_view session_id) {
    return store_->redeem_link_grant(grant, session_id);
}
std::optional<SessionContextId> SessionContextResolver::context_for_session(
    std::string_view session_id) {
    return store_->context_for_session(session_id);
}
bool SessionContextResolver::revoke_context_link(std::string_view session_id) {
    return store_->revoke_context_link(session_id);
}
void SessionContextResolver::put(const SessionContextId& context, std::string_view key,
                                 JsonValue value, std::optional<std::chrono::seconds> lifetime) {
    store_->put_context_value(context, key, std::move(value), lifetime);
}
std::optional<JsonValue> SessionContextResolver::get(
    const SessionContextId& context, std::string_view key) {
    return store_->get_context_value(context, key);
}
std::optional<JsonValue> SessionContextResolver::get_for_session(
    std::string_view session_id, std::string_view key) {
    return store_->get_context_value_for_session(session_id, key);
}

void SQLiteSessionStore::collect_expired(std::int64_t now) {
    if (now < 0) return;
    state_->write([now](sqlite3* db) {
        execute(db, "BEGIN IMMEDIATE");
        try {
            Statement links(db, "DELETE FROM session_context_links WHERE session_id IN "
                "(SELECT id FROM sessions WHERE expires_at<=?1 ORDER BY expires_at, id LIMIT 256)");
            sqlite_check(db, sqlite3_bind_int64(links.get(), 1, now), "Could not bind link cleanup time");
            sqlite_check(db, sqlite3_step(links.get()), "Could not collect expired context links");
            Statement statement(db, "DELETE FROM sessions WHERE id IN (SELECT id FROM sessions "
                "WHERE expires_at <= ?1 ORDER BY expires_at, id LIMIT 256)");
            sqlite_check(db, sqlite3_bind_int64(statement.get(), 1, now), "Could not bind expiration time");
            sqlite_check(db, sqlite3_step(statement.get()), "Could not collect expired SQLite sessions");
            Statement grants(db, "DELETE FROM session_context_grants WHERE rowid IN "
                "(SELECT rowid FROM session_context_grants WHERE expires_at<=?1 "
                "ORDER BY expires_at LIMIT 256)");
            sqlite_check(db, sqlite3_bind_int64(grants.get(), 1, now), "Could not bind grant cleanup time");
            sqlite_check(db, sqlite3_step(grants.get()), "Could not collect expired session grants");
            Statement values(db, "DELETE FROM session_context_values WHERE rowid IN "
                "(SELECT rowid FROM session_context_values WHERE expires_at>0 AND expires_at<=?1 "
                "ORDER BY expires_at LIMIT 256)");
            sqlite_check(db, sqlite3_bind_int64(values.get(), 1, now), "Could not bind context cleanup time");
            sqlite_check(db, sqlite3_step(values.get()), "Could not collect expired context values");
            execute(db, "COMMIT");
        } catch (...) {
            sqlite3_exec(db, "ROLLBACK", nullptr, nullptr, nullptr);
            throw;
        }
    });
}

} // namespace kotavel::http
