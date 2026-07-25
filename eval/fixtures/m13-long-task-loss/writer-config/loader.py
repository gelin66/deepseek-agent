from settings import Settings


def load_settings(environment: dict[str, str]) -> Settings:
    endpoint = environment.get("APP_ENDPOINT", "")
    retries = int(environment.get("APP_RETRIES", "3"))
    labels = tuple(
        tuple(item.split("="))
        for item in environment.get("APP_LABELS", "").split(",")
        if item
    )
    return Settings(endpoint=endpoint, retries=retries, labels=labels)
