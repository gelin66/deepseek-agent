import sys

from config_model import assign


def main(arguments: list[str]) -> int:
    values: dict[str, str] = {}
    for item in arguments:
        if not item.startswith("--set="):
            continue
        key, value = item.removeprefix("--set=").split("=")
        assign(values, key, value)
    print(values)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
