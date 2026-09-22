"""Check the generated tour against the docs-only Pages publication boundary.

Run: python3 -m unittest discover -s tools -p test_readme_links.py
"""
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit
import unittest

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"


class Links(HTMLParser):
    def __init__(self, text):
        super().__init__()
        self.hrefs = []
        self.feed(text)

    def handle_starttag(self, tag, attrs):
        if tag == "a":
            self.hrefs.extend(value for key, value in attrs if key == "href")


class PublishedLinks(unittest.TestCase):
    def test_generated_tour_links_resolve_inside_publication_or_repository(self):
        links = Links((DOCS / "README.html").read_text()).hrefs
        self.assertGreater(len(links), 20)
        for href in links:
            with self.subTest(href=href):
                url = urlsplit(href)
                if url.netloc == "github.com" and url.path.startswith("/sirmick/redoubt/"):
                    parts = url.path.split("/", 5)
                    self.assertIn(parts[3], ("blob", "tree"))
                    self.assertEqual(parts[4], "main")
                    self.assertTrue((ROOT / unquote(parts[5])).exists())
                elif not url.scheme and url.path:
                    target = (DOCS / unquote(url.path)).resolve()
                    self.assertTrue(target.is_relative_to(DOCS))
                    self.assertTrue(target.exists())


if __name__ == "__main__":
    unittest.main()
