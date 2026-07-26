import json
import sys
from typing import TextIO


def serve(input_stream: TextIO, output_stream: TextIO) -> int:
    for raw in input_stream:
        request = json.loads(raw)
        response = {
            "id": request["id"],
            "square": int(request["value"]) ** 2,
        }
        print(json.dumps(response), file=output_stream)
    return 0


def main() -> int:
    return serve(sys.stdin, sys.stdout)


if __name__ == "__main__":
    raise SystemExit(main())
