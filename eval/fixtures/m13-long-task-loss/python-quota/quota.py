def allocate_quota(total: int, weights: list[int], minimum: int = 0) -> list[int]:
    if not weights:
        return []
    remaining = total - minimum * len(weights)
    weight_sum = sum(weights)
    return [minimum + remaining * weight // weight_sum for weight in weights]
