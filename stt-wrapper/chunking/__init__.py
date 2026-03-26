"""Language-aware clause boundary detection for utterance chunking.

Forces STT to finalize long utterances at natural clause boundaries,
preventing audio/video desync in the translation pipeline.

The problem: if a host speaks for 5s but broadcast delay is 5s, the
pipeline (STT + translate + TTS) has 0s budget. Chunking breaks long
utterances into ~2s segments at clause boundaries so each chunk fits
within the delay budget.

To add a new language:
  1. Create a new file (e.g., spanish.py)
  2. Subclass ChunkDetector, override detect_boundary()
  3. Register it in DETECTOR_CLASSES below
"""

from __future__ import annotations

import time
from abc import ABC, abstractmethod

__all__ = ["ChunkDetector", "get_detector"]


class ChunkDetector(ABC):
    """Base class for language-specific clause boundary detection.

    Called on each STT interim. Returns True when the utterance should
    be force-finalized via Deepgram's Finalize message.

    Two triggers:
      1. Clause boundary detected AND min_duration elapsed
      2. Hard timeout at max_duration (regardless of boundary)
    """

    # Subclasses can override these defaults
    DEFAULT_MIN_DURATION: float = 1.5
    DEFAULT_MAX_DURATION: float = 3.0

    def __init__(
        self,
        min_duration: float | None = None,
        max_duration: float | None = None,
    ):
        self.min_duration = min_duration if min_duration is not None else self.DEFAULT_MIN_DURATION
        self.max_duration = max_duration if max_duration is not None else self.DEFAULT_MAX_DURATION
        self._start: float | None = None
        self._prev_text = ""

    def check(self, text: str) -> bool:
        """Check if we should force-finalize the current utterance.

        Call this on every interim transcript. Returns True once per
        chunk — the caller should send Finalize and then call reset().
        """
        now = time.monotonic()

        if self._start is None:
            self._start = now
            self._prev_text = text
            return False

        elapsed = now - self._start

        # Hard timeout: always finalize
        if elapsed >= self.max_duration:
            return True

        # Too early: need minimum audio for decent translation
        if elapsed < self.min_duration:
            self._prev_text = text
            return False

        # Language-specific clause boundary check
        result = self.detect_boundary(self._prev_text, text)
        self._prev_text = text
        return result

    def reset(self):
        """Reset state after a final is emitted (forced or natural)."""
        self._start = None
        self._prev_text = ""

    @abstractmethod
    def detect_boundary(self, prev_text: str, text: str) -> bool:
        """Detect if the current interim crosses a clause boundary.

        A boundary is "crossed" when the speaker has moved past a clause
        marker and started a new clause (i.e., there's text AFTER the
        marker, confirming the clause is complete).

        Args:
            prev_text: Previous interim text.
            text: Current interim text (full accumulated transcript).

        Returns:
            True if a clause boundary is detected with enough text after it.
        """
        ...


class FallbackDetector(ChunkDetector):
    """No clause detection — only hard timeout. Used for unknown languages."""

    def detect_boundary(self, prev_text: str, text: str) -> bool:
        return False


class _MarkerBasedDetector(ChunkDetector):
    """Shared logic for languages that use string markers for clause boundaries.

    Subclasses define CLAUSE_MARKERS (list of strings) and MIN_CHARS_AFTER
    (minimum characters after the marker to confirm a new clause started).
    """

    CLAUSE_MARKERS: list[str] = []
    MIN_CHARS_AFTER: int = 2

    def detect_boundary(self, prev_text: str, text: str) -> bool:
        for marker in self.CLAUSE_MARKERS:
            idx = text.rfind(marker)
            if idx == -1:
                continue
            after = text[idx + len(marker):].strip()
            if len(after) >= self.MIN_CHARS_AFTER:
                return True
        return False


# ── Detector Registry ─────────────────────────────────────

DETECTOR_CLASSES: dict[str, type[ChunkDetector]] = {}


def _register_builtins():
    """Lazy-import built-in detectors to keep module import fast."""
    if DETECTOR_CLASSES:
        return

    from .korean import KoreanDetector
    from .japanese import JapaneseDetector
    from .english import EnglishDetector
    from .chinese import ChineseDetector

    DETECTOR_CLASSES.update({
        "ko": KoreanDetector,
        "ja": JapaneseDetector,
        "en": EnglishDetector,
        "zh": ChineseDetector,
    })


def get_detector(lang: str, **kwargs) -> ChunkDetector:
    """Get the chunk detector for a language code.

    Falls back to FallbackDetector (timeout-only) for unknown languages.

    Args:
        lang: ISO 639-1 language code (e.g., "ko", "en", "ja", "zh").
        **kwargs: Passed to the detector constructor (min_duration, max_duration).
    """
    _register_builtins()
    cls = DETECTOR_CLASSES.get(lang, FallbackDetector)
    return cls(**kwargs)
