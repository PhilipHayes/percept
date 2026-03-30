#!/usr/bin/env python3
"""
spacy_parse.py — Read text from stdin, parse with spaCy, emit JSON to stdout.

Exit codes:
  0 — success
  1 — spaCy not installed
  2 — model not found
  3 — other parse failure
"""

import argparse
import json
import sys


def main():
    parser = argparse.ArgumentParser(description="Parse text with spaCy and emit JSON.")
    parser.add_argument("--model", default="en_core_web_sm", help="spaCy model to load")
    args = parser.parse_args()

    try:
        import spacy  # noqa: PLC0415
    except ImportError as e:
        print(
            json.dumps({"error": "spacy_not_installed", "message": str(e)}),
            file=sys.stderr,
        )
        sys.exit(1)

    try:
        nlp = spacy.load(args.model)
    except OSError as e:
        print(
            json.dumps({"error": "model_not_found", "message": str(e)}),
            file=sys.stderr,
        )
        sys.exit(2)

    try:
        text = sys.stdin.read()
        doc = nlp(text)

        sentences = []
        for sent in doc.sents:
            tokens = []
            for token in sent:
                tokens.append(
                    {
                        "text": token.text,
                        "lemma": token.lemma_,
                        "pos": token.pos_,
                        "tag": token.tag_,
                        "dep": token.dep_,
                        "head": token.head.i - sent.start,
                        "ent_type": token.ent_type_,
                        "ent_iob": token.ent_iob_,
                        "idx": token.idx,
                    }
                )
            sentences.append(
                {
                    "text": sent.text,
                    "start": sent.start_char,
                    "end": sent.end_char,
                    "tokens": tokens,
                }
            )

        entities = [
            {
                "text": ent.text,
                "label": ent.label_,
                "start_char": ent.start_char,
                "end_char": ent.end_char,
            }
            for ent in doc.ents
        ]

        result = {
            "text": text,
            "sentences": sentences,
            "entities": entities,
        }

        print(json.dumps(result))

    except Exception as e:  # noqa: BLE001
        print(
            json.dumps({"error": "parse_failed", "message": str(e)}),
            file=sys.stderr,
        )
        sys.exit(3)


if __name__ == "__main__":
    main()
