CREATE TABLE IF NOT EXISTS logs (
    level TEXT NOT NULL,
    module TEXT,
    file TEXT,
    line INTEGER,
    message TEXT NOT NULL,
    structured TEXT NOT NULL
);