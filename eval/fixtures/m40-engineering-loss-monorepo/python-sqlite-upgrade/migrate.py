import hashlib

from schema import TARGET_VERSION


def upgrade(connection):
    version = connection.execute("PRAGMA user_version").fetchone()[0]
    if version >= TARGET_VERSION:
        return
    connection.execute("ALTER TABLE records ADD COLUMN checksum TEXT")
    rows = connection.execute("SELECT id, payload FROM records").fetchall()
    for record_id, payload in rows:
        checksum = hashlib.sha256(payload.encode("utf-8")).hexdigest()
        connection.execute(
            "UPDATE records SET checksum = ? WHERE id = ?",
            (checksum, record_id),
        )
        connection.commit()
    connection.execute(f"PRAGMA user_version = {TARGET_VERSION}")
