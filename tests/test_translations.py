"""Catalog coverage and generated asset consistency, without extra dependencies."""
import json
import re
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

class Translations(unittest.TestCase):
    def test_literal_keys(self):
        keys = set()
        for path in (ROOT / 'src').rglob('*.rs'):
            if path.name == 'translations_generated.rs':
                continue
            for literal in re.findall(r'\btrf?\(\s*("(?:[^"\\]|\\.)*")', path.read_text()):
                keys.add(json.loads(literal))
        self.assertGreater(len(keys), 30)
        for code in 'de fr es it pt nl pl'.split():
            table = json.loads((ROOT / f'translations/{code}.json').read_text())
            table.update(json.loads((ROOT / f'translations/extra/{code}.json').read_text()))
            self.assertEqual(keys - table.keys(), set(), code)
            for key in keys:
                self.assertEqual(set(re.findall(r'\{\d+\}', key)), set(re.findall(r'\{\d+\}', table[key])), (code, key))

    def test_generated_assets(self):
        path = ROOT / 'src/translations_generated.rs'
        before = path.read_bytes()
        subprocess.run([str(ROOT / 'tools/gen-translations')], check=True)
        self.assertEqual(before, path.read_bytes())
