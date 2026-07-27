def upgrade(connection):
    connection.execute("PRAGMA user_version = 2")
