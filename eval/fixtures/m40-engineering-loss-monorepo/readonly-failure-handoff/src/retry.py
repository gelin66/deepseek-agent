from .cache import is_fresh


def should_retry(entry_time, now, ttl):
    return not is_fresh(entry_time, now, ttl)
