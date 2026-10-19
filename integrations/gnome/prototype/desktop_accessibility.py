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
