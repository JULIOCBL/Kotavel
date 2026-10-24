#!/usr/bin/env python3
"""Verify SQLite WAL sessions recover after an unclean process exit."""

from __future__ import annotations

import subprocess
import sqlite3
import sys
import tempfile
import time
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: session_crash_recovery.py SESSION_TEST_EXECUTABLE", file=sys.stderr)
        return 2

    executable = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="kotavel-session-crash-") as directory:
        database = Path(directory) / "sessions.sqlite3"
        subprocess.run(
            [str(executable), "--durable-crash-write", str(database)],
            check=True,
            timeout=15,
        )
        subprocess.run(
            [str(executable), "--durable-crash-read", str(database)],
            check=True,
            timeout=15,
        )
        with sqlite3.connect(database) as connection:
            plans = (
                ("sessions_expiry_idx",
                 "SELECT id FROM sessions WHERE expires_at<=? "
                 "ORDER BY expires_at, id LIMIT 256", (1,)),
                ("session_context_grants_expiry_idx",
                 "SELECT rowid FROM session_context_grants WHERE expires_at<=? "
                 "ORDER BY expires_at LIMIT 256", (1,)),
                ("session_context_grants_context_expiry_idx",
                 "SELECT grant_token FROM session_context_grants WHERE context_id=? AND expires_at>?",
                 ("0" * 64, 1)),
                ("session_context_values_expiry_idx",
                 "SELECT rowid FROM session_context_values WHERE expires_at>0 AND expires_at<=? "
                 "ORDER BY expires_at LIMIT 256", (1,)),
            )
            for expected_index, query, parameters in plans:
                description = " ".join(
                    row[3] for row in connection.execute("EXPLAIN QUERY PLAN " + query, parameters)
                )
                if expected_index not in description:
                    raise AssertionError(
                        f"expiration query does not use {expected_index}: {description}"
                    )

            context_ids = ("e" * 64, "f" * 64)
            connection.executemany(
                "INSERT INTO session_contexts(context_id, created_at) VALUES(?, 1)",
                ((context_id,) for context_id in context_ids),
            )
            connection.executemany(
                "INSERT INTO session_context_grants(grant_token, context_id, expires_at) "
                "VALUES(?, ?, 1)",
                ((f"{index:064x}", context_ids[index // 300]) for index in range(600)),
            )
            connection.executemany(
                "INSERT INTO session_context_values(context_id, key, data, expires_at) "
                "VALUES(?, ?, 'null', 1)",
                ((context_ids[0], f"expired-{index}") for index in range(300)),
            )
            session_ids = tuple(f"{1000 + index:064x}" for index in range(300))
            connection.executemany(
                "INSERT INTO sessions(id, created_at, last_activity, expires_at, revision, data) "
                "VALUES(?, 1, 1, 1, 1, '{}')",
                ((session_id,) for session_id in session_ids),
            )
            connection.executemany(
                "INSERT INTO session_context_links(session_id, context_id, linked_at) "
                "VALUES(?, ?, 1)",
                ((session_id, context_ids[0]) for session_id in session_ids),
            )
            connection.commit()

        subprocess.run(
            [str(executable), "--issue-link-grant", str(database), context_ids[0]],
            check=True,
            timeout=15,
        )
        with sqlite3.connect(database) as connection:
            expired_grants_after_issue = connection.execute(
                "SELECT count(*) FROM session_context_grants WHERE expires_at<=1"
            ).fetchone()[0]
            if expired_grants_after_issue != 344:
                raise AssertionError(
                    "grant issuance must remove at most 256 expired grants; "
                    f"{expired_grants_after_issue} expired rows remain"
                )

        subprocess.run(
            [str(executable), "--collect-expired", str(database), str(int(time.time()) + 10)],
            check=True,
            timeout=15,
        )
        with sqlite3.connect(database) as connection:
            remaining_grants = connection.execute(
                "SELECT count(*) FROM session_context_grants WHERE expires_at<=1"
            ).fetchone()[0]
            remaining_values = connection.execute(
                "SELECT count(*) FROM session_context_values WHERE expires_at<=1"
            ).fetchone()[0]
            remaining_sessions = connection.execute(
                "SELECT count(*) FROM sessions WHERE expires_at<=1"
            ).fetchone()[0]
            remaining_links = connection.execute(
                "SELECT count(*) FROM session_context_links"
            ).fetchone()[0]
            orphaned_links = connection.execute(
                "SELECT count(*) FROM session_context_links AS l "
                "LEFT JOIN sessions AS s ON s.id=l.session_id WHERE s.id IS NULL"
            ).fetchone()[0]
            if (remaining_grants != 88 or remaining_values != 44 or
                    remaining_sessions != 44 or remaining_links != 44 or orphaned_links != 0):
                raise AssertionError(
                    "one maintenance pass must remove at most 256 expired rows per table "
                    "without orphaning session-context links; "
                    f"remaining grants={remaining_grants}, values={remaining_values}, "
                    f"sessions={remaining_sessions}, links={remaining_links}, "
                    f"orphaned_links={orphaned_links}"
                )

    print("Durable recovery and bounded expiration cleanup passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
