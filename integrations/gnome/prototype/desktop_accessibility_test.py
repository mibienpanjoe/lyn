"""Regression for identical accessible names and WebKit's numeric role mismatch."""

import unittest

from desktop_accessibility import page_heading


class Node:
    def __init__(self, name, **attributes):
        self.name, self.attributes = name, attributes

    def get_name(self):
        return self.name

    def get_attributes(self):
        return self.attributes


def heading(name="All captures", **overrides):
    return Node(name, **({"toolkit": "WebKitGTK", "tag": "h1",
                         "computed-role": "heading", "level": "1"} | overrides))


class PageHeadingTest(unittest.TestCase):
    def test_selects_html_heading_among_same_named_button_and_section(self):
        title = heading()
        nodes = [Node("All captures", tag="button", **{"computed-role": "button"}),
                 Node("All captures", tag="section"), title]
        self.assertIs(page_heading(nodes, "All captures"), title)

    def test_duplicate_titles_are_unresolved(self):
        self.assertIsNone(page_heading([heading(), heading()], "All captures"))

    def test_different_page_and_missing_heading_are_unresolved(self):
        self.assertIsNone(page_heading([heading("Recent")], "All captures"))
        self.assertIsNone(page_heading([], "All captures"))

    def test_text_or_heading_like_nodes_cannot_substitute_for_page_title(self):
        for attributes in ({"tag": "button"}, {"computed-role": "button"},
                           {"level": "2"}, {"toolkit": "GTK"}, {"tag": ""}):
            with self.subTest(attributes=attributes):
                self.assertIsNone(page_heading([heading(**attributes)], "All captures"))


if __name__ == "__main__":
    unittest.main()
