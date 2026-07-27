def is_fresh(stored_at, now, ttl):
    return now - stored_at <= ttl
