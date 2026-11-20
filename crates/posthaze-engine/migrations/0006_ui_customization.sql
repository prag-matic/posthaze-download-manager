ALTER TABLE settings ADD COLUMN ui_table_density TEXT NOT NULL DEFAULT 'standard';
ALTER TABLE settings ADD COLUMN ui_show_speed_col INTEGER NOT NULL DEFAULT 1;
ALTER TABLE settings ADD COLUMN ui_show_eta_col INTEGER NOT NULL DEFAULT 1;
ALTER TABLE settings ADD COLUMN ui_show_segs_col INTEGER NOT NULL DEFAULT 1;
ALTER TABLE settings ADD COLUMN ui_show_toolbar_labels INTEGER NOT NULL DEFAULT 1;
ALTER TABLE settings ADD COLUMN ui_show_scrollbar_minimap INTEGER NOT NULL DEFAULT 1;
ALTER TABLE settings ADD COLUMN ui_show_titlebar_telemetry INTEGER NOT NULL DEFAULT 1;
ALTER TABLE settings ADD COLUMN ui_start_sidebar_collapsed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE settings ADD COLUMN ui_accent_override TEXT;
