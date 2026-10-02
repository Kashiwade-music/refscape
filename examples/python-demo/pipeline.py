"""Cross-file calls and nested declarations for canvas navigation."""

from model import Config, Counter, build_counter, load_config


def summarize(counter: Counter, config: Config) -> str:
    """Open annotate to see its containing function declaration."""

    def annotate(value: int) -> str:
        """Format a value using the enclosing function's typed settings."""
        return f"{config.title}: {value}"

    value: int = counter.increment(config.step)
    return annotate(value)


def run(config: Config) -> str:
    """Click counter to highlight uses and navigate to its Counter type."""
    counter: Counter = build_counter(config)
    result: str = summarize(counter, config)
    return result


def preview() -> str:
    """A second cross-file caller of load_config and run."""
    config: Config = load_config()
    return run(config)
