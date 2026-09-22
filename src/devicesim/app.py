from __future__ import annotations

import sys


def main() -> None:
    argv = sys.argv[1:]
    if argv and argv[0] in {"-h", "--help", "--version", "gui", "run", "validate", "schema"}:
        from devicesim.cli import cli_main

        raise SystemExit(cli_main(argv))
    from devicesim.cli import cli_main

    raise SystemExit(cli_main(["gui", *argv]))


if __name__ == "__main__":
    main()
