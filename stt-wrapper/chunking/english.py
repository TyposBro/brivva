"""English clause boundary detection.

English is SVO with clear clause markers:
  Commas before coordinating conjunctions (and, but, or, so)
  Commas before subordinating conjunctions (because, since, although)
  Semicolons between independent clauses
"""

from . import _MarkerBasedDetector


class EnglishDetector(_MarkerBasedDetector):
    DEFAULT_MIN_DURATION = 1.5
    DEFAULT_MAX_DURATION = 3.0

    CLAUSE_MARKERS = [
        # Comma + coordinating conjunction
        ", and ", ", but ", ", or ", ", so ",
        ", yet ", ", nor ",
        # Comma + subordinating conjunction
        ", because ", ", since ", ", although ",
        ", while ", ", whereas ", ", unless ",
        # Comma + relative pronoun
        ", which ", ", where ", ", when ",
        # Comma + adverbial connector
        ", however ", ", therefore ", ", meanwhile ",
        # Semicolons
        "; ",
        # Sentence connectors (common in spoken English — no pause between sentences)
        ". And ", ". But ", ". So ", ". However ",
        ". Also ", ". Then ", ". Now ",
    ]

    MIN_CHARS_AFTER = 3  # English needs a few characters to confirm new clause
