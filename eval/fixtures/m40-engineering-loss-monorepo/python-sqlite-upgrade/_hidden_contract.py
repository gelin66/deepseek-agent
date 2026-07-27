import hashlib
import sqlite3

from migrate import upgrade
from schema import record_columns

connection = sqlite3.connect(":memory:")
connection.execute("CREATE TABLE records(id INTEGER PRIMARY KEY, payload TEXT NOT NULL)")
connection.execute("INSERT INTO records(payload) VALUES ('海豚')")
connection.execute("PRAGMA user_version = 1")
upgrade(connection)
columns = tuple(row[1] for row in connection.execute("PRAGMA table_info(records)"))
assert columns == record_columns()
row = connection.execute("SELECT id, payload, checksum FROM records").fetchone()
assert row == (1, "海豚", hashlib.sha256("海豚".encode("utf-8")).hexdigest())
assert connection.execute("PRAGMA user_version").fetchone()[0] == 3
upgrade(connection)

broken = sqlite3.connect(":memory:")
broken.execute("CREATE TABLE records(id INTEGER PRIMARY KEY, payload BLOB NOT NULL)")
broken.execute("INSERT INTO records(payload) VALUES (x'ff')")
broken.execute("PRAGMA user_version = 1")
broken.commit()
try:
    upgrade(broken)
except (UnicodeDecodeError, AttributeError):
    pass
else:
    raise AssertionError("invalid payload must fail")
columns = tuple(row[1] for row in broken.execute("PRAGMA table_info(records)"))
assert columns == ("id", "payload")
assert broken.execute("PRAGMA user_version").fetchone()[0] == 1
