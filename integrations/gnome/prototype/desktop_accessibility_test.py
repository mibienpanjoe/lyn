"""Regression for identical accessible names and WebKit's numeric role mismatch."""

import unittest

from desktop_accessibility import page_heading, html_alert, library_detail_scope


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


class LibraryDetailScopeTest(unittest.TestCase):
    def detail(self, **overrides):
        return Node("Screenshot", **({"toolkit": "WebKitGTK", "tag": "h2", "id": "capture-detail-title",
                                    "computed-role": "heading", "level": "2"} | overrides))

    def back(self, name="Back to All captures", **overrides):
        return Node(name, **({"toolkit": "WebKitGTK", "tag": "button", "computed-role": "button"} | overrides))

    def test_wide_detail_requires_selected_detail_and_exact_scope_heading(self):
        scope = heading()
        self.assertIs(library_detail_scope([scope, self.detail()], "All captures", "Screenshot"), scope)
        self.assertIsNone(library_detail_scope([scope], "All captures", "Screenshot"))

    def test_compact_detail_uses_exact_html_back_control(self):
        back = self.back()
        self.assertIs(library_detail_scope([back, self.detail()], "All captures", "Screenshot"), back)
        self.assertIsNone(library_detail_scope([self.back("All captures"), self.detail()], "All captures", "Screenshot"))

    def test_voice_detail_requires_its_own_kind(self):
        back = self.back()
        voice = Node("Voice note", **self.detail().attributes)
        self.assertIs(library_detail_scope([back, voice], "All captures", "Voice note"), back)
        self.assertIsNone(library_detail_scope([back, voice], "All captures", "Screenshot"))

    def test_wrong_scope_kind_or_missing_detail_refuses(self):
        for nodes in ([self.back("Back to Recent"), self.detail()], [self.back()],
                      [self.back(), Node("Voice note", **self.detail().attributes)]):
            with self.subTest(nodes=nodes):
                self.assertIsNone(library_detail_scope(nodes, "All captures", "Screenshot"))

    def test_lookalike_back_and_detail_nodes_refuse(self):
        for override in ({"tag": "span"}, {"computed-role": "link"}, {"toolkit": "GTK"}):
            with self.subTest(override=override):
                self.assertIsNone(library_detail_scope([self.back(**override), self.detail()], "All captures", "Screenshot"))
        for override in ({"id": "other"}, {"tag": "h1"}, {"level": "1"}, {"toolkit": "GTK"}):
            with self.subTest(override=override):
                self.assertIsNone(library_detail_scope([self.back(), self.detail(**override)], "All captures", "Screenshot"))

    def test_ambiguous_scope_or_selected_detail_refuses(self):
        for nodes in ([heading(), heading(), self.detail(), self.back()],
                      [self.back(), self.back(), self.detail()], [self.back(), self.detail(), self.detail()]):
            with self.subTest(nodes=nodes):
                self.assertIsNone(library_detail_scope(nodes, "All captures", "Screenshot"))


class HtmlAlertTest(unittest.TestCase):
    def alert(self, **overrides):
        return Node("", **({"toolkit": "WebKitGTK", "tag": "div", "id": "capture-error",
                            "computed-role": "alert"} | overrides))

    def test_unnamed_exact_html_alert_is_selected(self):
        alert = self.alert()
        self.assertIs(html_alert([Node("stale", tag="span"), alert], "capture-error"), alert)

    def test_duplicates_and_wrong_semantics_are_refused(self):
        self.assertIsNone(html_alert([], "capture-error"))
        self.assertIsNone(html_alert([self.alert(), self.alert()], "capture-error"))
        for attributes in ({"tag": "button"}, {"computed-role": "button"},
                           {"id": "unrelated"}, {"toolkit": "GTK"}):
            with self.subTest(attributes=attributes):
                self.assertIsNone(html_alert([self.alert(**attributes)], "capture-error"))


if __name__ == "__main__":
    unittest.main()
