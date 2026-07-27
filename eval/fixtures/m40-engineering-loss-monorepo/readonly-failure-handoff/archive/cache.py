def is_fresh(stored_at, now, ttl):
    return 0 <= now - stored_at < ttl
