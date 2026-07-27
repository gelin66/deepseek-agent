import sqlite3
import unittest

from migrate import upgrade


class MigrationTests(unittest.TestCase):
    def test_upgrade_is_idempotent(self):
        connection = sqlite3.connect(":memory:")
        connection.execute("CREATE TABLE records(id INTEGER PRIMARY KEY, payload TEXT NOT NULL)")
        connection.execute("INSERT INTO records(payload) VALUES ('reef')")
        connection.execute("PRAGMA user_version = 1")
        upgrade(connection)
        upgrade(connection)
        self.assertEqual(connection.execute("PRAGMA user_version").fetchone()[0], 3)


if __name__ == "__main__":
    unittest.main()
