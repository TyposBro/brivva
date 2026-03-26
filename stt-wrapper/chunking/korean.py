"""Korean clause boundary detection.

Korean is SOV — the verb (and therefore the meaning) comes at the end.
Clause boundaries are marked by connective endings (연결어미) and
sentence-level conjunctions (접속사).

Reliable boundary signals:
  ~는데(요),  ~고(요),  ~거든(요),  ~니까(요),  ~지만
  ~때문에,    ~면서,    ~어서/아서
  그리고, 그런데, 그래서, 하지만, 그래도

These are safe to split on because each side of the boundary is a
grammatically complete clause that translates independently.
"""

from . import _MarkerBasedDetector


class KoreanDetector(_MarkerBasedDetector):
    # Korean speakers use frequent clause markers, so we can chunk earlier
    DEFAULT_MIN_DURATION = 1.0
    DEFAULT_MAX_DURATION = 2.5

    # Longer patterns first to avoid partial matches.
    # Each marker includes trailing punctuation/space to confirm the clause
    # ended (not just a substring match in the middle of a word).
    CLAUSE_MARKERS = [
        # ~는데(요) — after verbs (하는데요), adjectives use 은데(요) (좋은데요)
        "는데요,", "은데요,", "인데요,",
        "는데요 ", "은데요 ", "인데요 ",
        "는데,", "은데,", "인데,",
        "는데 ", "은데 ", "인데 ",
        # ~거든(요)
        "거든요,", "거든요 ",
        "거든,", "거든 ",
        # ~니까(요)
        "니까요,", "니까요 ",
        "니까,", "니까 ",
        # ~고(요)
        "고요,", "구요,",
        "고요 ", "구요 ",
        # Other connective endings
        "지만,", "지만 ",
        "때문에,", "때문에 ",
        "면서,", "면서 ",
        "어서,", "아서,", "어서 ", "아서 ",
        "하고,", "하고 ",
        # Conjunctions (접속사) — appear between clauses
        " 그리고 ", " 그런데 ", " 그래서 ", " 하지만 ",
        " 그래도 ", " 그러면 ", " 그러니까 ",
        " 또한 ", " 그다음에 ",
    ]

    MIN_CHARS_AFTER = 2  # Korean chars carry more meaning per character
