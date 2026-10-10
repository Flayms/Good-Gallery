import { sql } from 'drizzle-orm'
import {
  type AnySQLiteColumn,
  index,
  integer,
  primaryKey,
  real,
  sqliteTable,
  text,
  uniqueIndex,
} from 'drizzle-orm/sqlite-core'

// All timestamps are integer milliseconds since the Unix epoch.

export const libraryRoots = sqliteTable('library_roots', {
  id: integer('id').primaryKey({ autoIncrement: true }),
  path: text('path').notNull().unique(),
  label: text('label').notNull(),
  status: text('status', { enum: ['unknown', 'online', 'offline'] })
    .notNull()
    .default('unknown'),
  lastScanAt: integer('last_scan_at'),
})

export const media = sqliteTable(
  'media',
  {
    id: integer('id').primaryKey({ autoIncrement: true }),
    rootId: integer('root_id')
      .notNull()
      .references(() => libraryRoots.id, { onDelete: 'cascade' }),
    relPath: text('rel_path').notNull(),
    fileName: text('file_name').notNull(),
    kind: text('kind', { enum: ['image', 'video'] }).notNull(),
    size: integer('size').notNull(),
    mtime: integer('mtime').notNull(),
    /** Mtime of the `.xmp` sidecar, part of change detection. */
    sidecarMtime: integer('sidecar_mtime'),
    width: integer('width'),
    height: integer('height'),
    /** Seconds. */
    duration: real('duration'),
    takenAt: integer('taken_at'),
    sortDate: integer('sort_date').notNull().generatedAlwaysAs(sql`coalesce(taken_at, mtime)`, { mode: 'virtual' }),
    /** Folder of `relPath` with a trailing `/` (`''` at the root): strips everything after the last `/`. */
    dir: text('dir').notNull().generatedAlwaysAs(sql`rtrim(rel_path, replace(rel_path, '/', ''))`, { mode: 'virtual' }),
    /** Case-insensitive name for sorting (an expression index would break drizzle-kit). */
    fileNameLower: text('file_name_lower').notNull().generatedAlwaysAs(sql`lower(file_name)`, { mode: 'virtual' }),
    /** Base64-encoded ThumbHash. */
    thumbhash: text('thumbhash'),
    thumbStatus: text('thumb_status', { enum: ['pending', 'ready', 'error'] })
      .notNull()
      .default('pending'),
    /** 1-5, or null when unrated. */
    rating: integer('rating'),
    /** Metadata extractor version that last wrote this row; lets re-reads pick up new fields without a full rescan. */
    metaVersion: integer('meta_version').notNull().default(0),
  },
  (t) => [
    uniqueIndex('media_root_path_uq').on(t.rootId, t.relPath),
    index('media_date_idx').on(t.sortDate, t.id),
    index('media_root_date_idx').on(t.rootId, t.sortDate, t.id),
    index('media_name_idx').on(t.fileNameLower, t.id),
    index('media_mtime_idx').on(t.mtime, t.id),
    index('media_size_idx').on(t.size, t.id),
    index('media_dir_idx').on(t.rootId, t.dir),
    // Background thumbnail generation picks pending media newest first.
    index('media_thumb_idx').on(t.thumbStatus, t.sortDate),
    index('media_rating_idx').on(t.rating, t.id),
  ],
)

/**
 * Folders of a root as last listed, with their mtime before listing. A folder's mtime changes when entries are
 * added, removed or renamed in it (not when files change), so quick scans only re-list folders whose mtime differs.
 */
export const folders = sqliteTable(
  'folders',
  {
    rootId: integer('root_id')
      .notNull()
      .references(() => libraryRoots.id, { onDelete: 'cascade' }),
    /** `/`-separated, no trailing `/`; `''` for the root itself. */
    relDir: text('rel_dir').notNull(),
    mtime: integer('mtime').notNull(),
  },
  (t) => [primaryKey({ columns: [t.rootId, t.relDir] })],
)

export const tags = sqliteTable(
  'tags',
  {
    id: integer('id').primaryKey({ autoIncrement: true }),
    name: text('name').notNull(),
    nameNorm: text('name_norm').notNull().unique(),
    parentId: integer('parent_id').references((): AnySQLiteColumn => tags.id, { onDelete: 'cascade' }),
    /** Number of `media_tags` rows (direct links only), maintained by triggers. */
    mediaCount: integer('media_count').notNull().default(0),
  },
  (t) => [index('tags_parent_idx').on(t.parentId)],
)

export const mediaTags = sqliteTable(
  'media_tags',
  {
    mediaId: integer('media_id')
      .notNull()
      .references(() => media.id, { onDelete: 'cascade' }),
    tagId: integer('tag_id')
      .notNull()
      .references(() => tags.id, { onDelete: 'cascade' }),
  },
  (t) => [primaryKey({ columns: [t.mediaId, t.tagId] }), index('media_tags_tag_idx').on(t.tagId, t.mediaId)],
)

/** User settings as JSON values, validated on read (`main/settings.ts`). */
export const settings = sqliteTable('settings', {
  key: text('key').primaryKey(),
  value: text('value', { mode: 'json' }).notNull(),
})

export type LibraryRoot = typeof libraryRoots.$inferSelect
export type Media = typeof media.$inferSelect
export type Tag = typeof tags.$inferSelect
