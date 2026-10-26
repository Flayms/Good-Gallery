// Writes `reference.db`: an index created by the reference app's own migrator (drizzle + better-sqlite3), with a few
// rows in every table. Tests open it to prove that an index written by the Electron version keeps working (REQ-001-08).
//
// Regenerate (only needed when the reference adds a migration):
//   node crates/db/tests/fixtures/make-reference-db.mjs ../good-gallery
import { rmSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const reference = resolve(process.argv[2] ?? '../good-gallery')
const require = createRequire(join(reference, 'package.json'))
const Database = require('better-sqlite3')
const { drizzle } = await import(pathToFileURL(require.resolve('drizzle-orm/better-sqlite3')).href)
const { migrate } = await import(pathToFileURL(require.resolve('drizzle-orm/better-sqlite3/migrator')).href)

const out = join(dirname(fileURLToPath(import.meta.url)), 'reference.db')
rmSync(out, { force: true })
const sqlite = new Database(out)
migrate(drizzle({ client: sqlite }), { migrationsFolder: join(reference, 'drizzle') })

sqlite.exec(`
  INSERT INTO library_roots (id, path, label, status, last_scan_at) VALUES
    (1, 'D:\\Photos', 'Photos', 'online', 1790000000000),
    (2, '\\\\nas\\archive', 'archive', 'offline', NULL);
  INSERT INTO media (id, root_id, rel_path, file_name, kind, size, mtime, width, height, duration, taken_at,
                     thumbhash, thumb_status, sidecar_mtime, rating, meta_version) VALUES
    (1, 1, '2024/IMG_0001.jpg', 'IMG_0001.jpg', 'image', 123456, 1700000000000, 4000, 3000, NULL, 1690000000000,
     'YJqGPQw7sFlslqhFafSE+Q6oJ1h2iHB2Rw', 'ready', NULL, 4, 3),
    (2, 1, 'clip.mp4', 'clip.mp4', 'video', 9876543, 1710000000000, 1920, 1080, 12.5, NULL, NULL, 'pending', NULL,
     NULL, 3),
    (3, 2, 'old/scan.png', 'scan.png', 'image', 2048, 1600000000000, NULL, NULL, NULL, NULL, NULL, 'error',
     1600000001000, NULL, 2);
  INSERT INTO tags (id, name, name_norm, parent_id) VALUES
    (1, 'Places', 'places', NULL), (2, 'France', 'france', 1), (3, 'Paris', 'paris', 2), (4, 'Sunset', 'sunset', NULL);
  INSERT INTO media_tags (media_id, tag_id) VALUES (1, 3), (1, 4), (2, 4);
  INSERT INTO folders (root_id, rel_dir, mtime) VALUES (1, '', 1700000000000), (1, '2024', 1700000000000);
  INSERT INTO settings (key, value) VALUES ('thumbCacheGiB', '10'), ('quickCheckIntervalMinutes', '15'),
    ('ioConcurrency', '4');
`)
sqlite.pragma('journal_mode = DELETE')
sqlite.close()
console.log(`wrote ${out}`)
