"""Japanese clause boundary detection.

Japanese is SOV like Korean. Clause boundaries are marked by:
  が、 けど、 から、 ので、 て/で form, し、
  Conjunctions: そして、でも、だから、しかし、それから
"""

from . import _MarkerBasedDetector


class JapaneseDetector(_MarkerBasedDetector):
    DEFAULT_MIN_DURATION = 1.0
    DEFAULT_MAX_DURATION = 2.5

    CLAUSE_MARKERS = [
        # Clause-ending particles + comma (most reliable)
        "けれども、", "けれど、", "けども、",
        "けど、", "けど ",
        "ですが、", "ですが ", "ますが、", "ますが ",
        "ですけど、", "ですけど ",
        "ので、", "ので ", "から、", "から ",
        "だけど、", "だけど ",
        "ますし、", "ますし ", "ですし、", "ですし ",
        # Te-form (continuation)
        "しまして、", "しまして ",
        "まして、", "まして ",
        "して、", "して ",
        "って、", "って ",
        "んで、", "んで ",
        # Conjunctions
        "そして ", "でも ", "だから ", "しかし ",
        "それから ", "ところが ", "それで ", "なので ",
        "それに ", "また ",
    ]

    MIN_CHARS_AFTER = 2
