ALTER TABLE settings ADD COLUMN category_url_rule_precedence INTEGER NOT NULL DEFAULT 1;
ALTER TABLE settings ADD COLUMN set_file_time_from_server INTEGER NOT NULL DEFAULT 0;
ALTER TABLE downloads ADD COLUMN category_id TEXT;

CREATE TABLE IF NOT EXISTS download_categories (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    extensions TEXT NOT NULL DEFAULT '',
    url_patterns TEXT NOT NULL DEFAULT '',
    output_dir TEXT,
    remember_last_folder INTEGER NOT NULL DEFAULT 0,
    is_builtin INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0
);

INSERT OR IGNORE INTO download_categories (
    id, name, extensions, url_patterns, output_dir, remember_last_folder, is_builtin, sort_order
) VALUES
    ('archives', 'Archives', 'zip,7z,rar,tar,gz,tar.gz,tgz,xz,tar.xz,zst,tar.zst,bz2,iso', '', NULL, 0, 1, 10),
    ('video', 'Video', 'mp4,mkv,webm,mov,avi,flv,m4v,wmv', '', NULL, 0, 1, 20),
    ('audio', 'Audio', 'mp3,flac,wav,aac,ogg,m4a,opus,wma', '', NULL, 0, 1, 30),
    ('documents', 'Documents', 'pdf,doc,docx,xls,xlsx,ppt,pptx,txt,md,csv,epub', '', NULL, 0, 1, 40),
    ('programs', 'Programs', 'exe,msi,msix,appx,bat,ps1,cmd', '', NULL, 0, 1, 50),
    ('general', 'General', '', '', NULL, 0, 1, 9999);
