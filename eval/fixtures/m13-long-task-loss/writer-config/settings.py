from dataclasses import dataclass


@dataclass
class Settings:
    endpoint: str
    retries: int = 3
    labels: tuple[tuple[str, str], ...] = ()
