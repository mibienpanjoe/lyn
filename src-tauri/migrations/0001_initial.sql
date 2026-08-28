CREATE TABLE contexts (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('project', 'standalone')),
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    project_key TEXT,
    project_path TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (
        kind = 'project'
        OR (project_key IS NULL AND project_path IS NULL)
    )
);

CREATE UNIQUE INDEX contexts_project_key_unique
    ON contexts(project_key)
    WHERE project_key IS NOT NULL;

CREATE TABLE captures (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL UNIQUE,
    context_id TEXT NOT NULL REFERENCES contexts(id),
    kind TEXT NOT NULL CHECK (kind IN ('text', 'image', 'audio')),
    text_body TEXT,
    caption TEXT,
    caption_source TEXT CHECK (
        caption_source IS NULL
        OR caption_source IN ('user', 'context_generated', 'transcript_generated')
    ),
    caption_revision INTEGER NOT NULL DEFAULT 0 CHECK (caption_revision >= 0),
    branch_name TEXT,
    source_app TEXT,
    source_window_title TEXT,
    captured_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (
        (kind = 'text'
            AND text_body IS NOT NULL
            AND length(trim(text_body)) > 0
            AND caption IS NULL
            AND caption_source IS NULL)
        OR
        (kind IN ('image', 'audio')
            AND text_body IS NULL
            AND (
                (caption IS NULL AND caption_source IS NULL)
                OR (caption IS NOT NULL
                    AND length(trim(caption)) > 0
                    AND caption_source IS NOT NULL)
            ))
    )
);

CREATE INDEX captures_chronology
    ON captures(captured_at DESC, id DESC);
CREATE INDEX captures_context_chronology
    ON captures(context_id, captured_at DESC, id DESC);
CREATE INDEX captures_context_branch_chronology
    ON captures(context_id, branch_name, captured_at DESC, id DESC);

CREATE TABLE media_assets (
    id TEXT PRIMARY KEY NOT NULL,
    capture_id TEXT NOT NULL UNIQUE REFERENCES captures(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('image', 'audio')),
    relative_path TEXT NOT NULL UNIQUE CHECK (
        length(relative_path) > 0
        AND relative_path NOT LIKE '/%'
        AND relative_path NOT LIKE '\%'
        AND instr(relative_path, '../') = 0
        AND instr(relative_path, '..\') = 0
    ),
    mime_type TEXT NOT NULL CHECK (mime_type IN ('image/png', 'audio/wav')),
    byte_size INTEGER NOT NULL CHECK (byte_size > 0),
    checksum TEXT NOT NULL CHECK (length(checksum) > 0),
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    width_px INTEGER CHECK (width_px IS NULL OR width_px > 0),
    height_px INTEGER CHECK (height_px IS NULL OR height_px > 0),
    created_at TEXT NOT NULL,
    CHECK (
        (kind = 'audio'
            AND mime_type = 'audio/wav'
            AND duration_ms IS NOT NULL
            AND width_px IS NULL
            AND height_px IS NULL)
        OR
        (kind = 'image'
            AND mime_type = 'image/png'
            AND duration_ms IS NULL)
    )
);

CREATE TRIGGER media_assets_kind_matches_capture_insert
BEFORE INSERT ON media_assets
FOR EACH ROW
WHEN (SELECT kind FROM captures WHERE id = NEW.capture_id) IS NOT NEW.kind
BEGIN
    SELECT RAISE(ABORT, 'media kind does not match capture kind');
END;

CREATE TRIGGER media_assets_kind_matches_capture_update
BEFORE UPDATE OF capture_id, kind ON media_assets
FOR EACH ROW
WHEN (SELECT kind FROM captures WHERE id = NEW.capture_id) IS NOT NEW.kind
BEGIN
    SELECT RAISE(ABORT, 'media kind does not match capture kind');
END;

CREATE TABLE enrichment_jobs (
    id TEXT PRIMARY KEY NOT NULL,
    capture_id TEXT NOT NULL REFERENCES captures(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('context_caption', 'speech_caption')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'completed', 'skipped', 'failed')),
    input_revision INTEGER NOT NULL CHECK (input_revision >= 0),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    last_error_code TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (capture_id, kind)
);

CREATE INDEX enrichment_jobs_queue
    ON enrichment_jobs(status, updated_at, id);

CREATE VIRTUAL TABLE captures_fts USING fts5(
    capture_id UNINDEXED,
    search_text,
    tokenize = 'unicode61'
);

CREATE TRIGGER captures_fts_insert
AFTER INSERT ON captures
BEGIN
    INSERT INTO captures_fts(capture_id, search_text)
    VALUES (NEW.id, COALESCE(NEW.text_body, NEW.caption, ''));
END;

CREATE TRIGGER captures_fts_update
AFTER UPDATE OF text_body, caption ON captures
BEGIN
    UPDATE captures_fts
    SET search_text = COALESCE(NEW.text_body, NEW.caption, '')
    WHERE capture_id = NEW.id;
END;

CREATE TRIGGER captures_fts_delete
AFTER DELETE ON captures
BEGIN
    DELETE FROM captures_fts WHERE capture_id = OLD.id;
END;

CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL CHECK (length(trim(key)) > 0),
    value_json TEXT NOT NULL CHECK (json_valid(value_json)),
    updated_at TEXT NOT NULL
);
