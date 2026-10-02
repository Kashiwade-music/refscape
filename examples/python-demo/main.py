"""Entry point for a standard-library-only Refscape Python demo."""

from model import Config, load_config
from pipeline import run


def main() -> None:
    """Follow load_config and run into their definitions in other files."""
    config: Config = load_config()
    print(run(config))


if __name__ == "__main__":
    main()
