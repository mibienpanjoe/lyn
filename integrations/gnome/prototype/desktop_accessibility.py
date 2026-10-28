"""Select the WebKit page title without reinterpreting AT-SPI role numbers."""


def page_heading(nodes, title):
    candidates = []
    for node in nodes:
        if node.get_name() != title:
            continue
        attributes = node.get_attributes()
        if all(attributes.get(key) == value for key, value in (
                ("toolkit", "WebKitGTK"), ("tag", "h1"),
                ("computed-role", "heading"), ("level", "1"))):
            candidates.append(node)
    return candidates[0] if len(candidates) == 1 else None


def html_alert(nodes, element_id):
    candidates = []
    for node in nodes:
        attributes = node.get_attributes()
        if all(attributes.get(key) == value for key, value in (
                ("toolkit", "WebKitGTK"), ("tag", "div"),
                ("id", element_id), ("computed-role", "alert"))):
            candidates.append(node)
    return candidates[0] if len(candidates) == 1 else None


def library_detail_scope(nodes, title, kind):
    """Require the selected detail plus its wide or compact scope control."""
    detail = [node for node in nodes if node.get_name() == kind and all(
        node.get_attributes().get(key) == value for key, value in (
            ("toolkit", "WebKitGTK"), ("tag", "h2"), ("computed-role", "heading"),
            ("level", "2"), ("id", "capture-detail-title")))]
    if len(detail) != 1:
        return None
    headings = [node for node in nodes if node.get_name() == title and all(
        node.get_attributes().get(key) == value for key, value in (
            ("toolkit", "WebKitGTK"), ("tag", "h1"),
            ("computed-role", "heading"), ("level", "1")))]
    if headings:
        return headings[0] if len(headings) == 1 else None
    back = [node for node in nodes if node.get_name() == f"Back to {title}" and all(
        node.get_attributes().get(key) == value for key, value in (
            ("toolkit", "WebKitGTK"), ("tag", "button"), ("computed-role", "button")))]
    return back[0] if len(back) == 1 else None
