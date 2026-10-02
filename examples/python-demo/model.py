"""Typed values shared by the Python navigation demo."""


class Config:
    """Settings consumed by the report pipeline."""

    def __init__(self, title: str, step: int) -> None:
        self.title: str = title
        self.step: int = step


class Counter:
    """A counter whose methods demonstrate containing class declarations."""

    def __init__(self, value: int = 0) -> None:
        self.value: int = value

    def increment(self, step: int) -> int:
        """Advance the counter and return its new value."""
        self.value += step
        return self.value


def load_config() -> Config:
    """Find callers in both main.py and pipeline.py."""
    return Config("Refscape Python demo", 2)


def build_counter(config: Config) -> Counter:
    """Construct a typed counter using settings from another declaration."""
    return Counter(config.step)
