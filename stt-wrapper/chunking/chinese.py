"""Chinese clause boundary detection.

Chinese uses comma-separated clauses and conjunctions:
  ，(Chinese comma) between clauses
  但是、因为、所以、然后、而且、不过、可是、虽然
"""

from . import _MarkerBasedDetector


class ChineseDetector(_MarkerBasedDetector):
    DEFAULT_MIN_DURATION = 1.0
    DEFAULT_MAX_DURATION = 3.0

    CLAUSE_MARKERS = [
        # Conjunction after Chinese comma (strongest signals)
        "，但是", "，因为", "，所以", "，然后",
        "，而且", "，不过", "，可是", "，虽然",
        "，如果", "，因此", "，于是", "，而",
        "，同时", "，另外", "，接着",
        # Same with ASCII comma (STT sometimes uses either)
        ",但是", ",因为", ",所以", ",然后",
        ",而且", ",不过", ",可是",
        # Plain Chinese comma (weaker signal but still a clause break)
        "，",
    ]

    MIN_CHARS_AFTER = 2
