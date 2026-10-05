import { and, desc, eq, gte, inArray, lt, ne, notInArray, or, type SQL, sql } from 'drizzle-orm'
import type { Db } from '../main/db'
import { folders, type LibraryRoot, libraryRoots, media, mediaTags, tags } from '../main/db/schema'
import { METADATA_VERSION } from '../shared/metadata'
import { cleanTag, normalizeTag } from '../shared/tags'
import type { MediaKind } from './media-types'
import type { MediaMetadata } from './metadata'
import type { ThumbKey } from './thumb-cache'

type Tx = Parameters<Parameters<Db['transaction']>[0]>[0]

export interface ExistingEntry {
  id: number
  size: number
  mtime: number
  sidecarMtime: number | null
  metaVersion: number
}

export interface IndexedFile {
  relPath: string
  fileName: string
  kind: MediaKind
  size: number
  mtime: number
  sidecarMtime: number | null
  metadata: MediaMetadata
}

/** Everything needed to render a thumbnail and to store it under the right cache key. */
export interface ThumbSource extends ThumbKey {
  id: number
  rootPath: string
  kind: MediaKind
  duration: number | null
}

const DELETE_CHUNK = 500
const INSERT_CHUNK = 500

/** Range condition for the subfolders of `relDir` (`/`-separated, no trailing `/`): `0` is the character after `/`. */
function subfolderRange(column: typeof media.dir | typeof folders.relDir, relDir: string): SQL | undefined {
  return and(gte(column, `${relDir}/`), lt(column, `${relDir}0`))
}

/** All index writes of the indexer process. Not safe for concurrent use from multiple processes. */
export class IndexWriter {
  readonly #db: Db
  // Caches over `tags`; cleared whenever a write might have been rolled back or rows were deleted.
  readonly #tagIds = new Map<string, number>()
  readonly #parents = new Map<number, number | null>()

  constructor(db: Db) {
    this.#db = db
  }

  root(rootId: number): LibraryRoot | undefined {
    return this.#db.select().from(libraryRoots).where(eq(libraryRoots.id, rootId)).get()
  }

  setRootStatus(rootId: number, status: LibraryRoot['status'], lastScanAt?: number): void {
    this.#db.update(libraryRoots).set({ status, lastScanAt }).where(eq(libraryRoots.id, rootId)).run()
  }

  /** Indexed files of a root, keyed by `relPath`. */
  existing(rootId: number): Map<string, ExistingEntry> {
    return this.#existing(eq(media.rootId, rootId))
  }

  /** Indexed files directly in one folder of a root, keyed by `relPath`. */
  existingIn(rootId: number, relDir: string): Map<string, ExistingEntry> {
    return this.#existing(and(eq(media.rootId, rootId), eq(media.dir, relDir ? `${relDir}/` : '')))
  }

  /** Folder mtimes of a root as last listed, keyed by `relDir`. */
  folders(rootId: number): Map<string, number> {
    const rows = this.#db.select().from(folders).where(eq(folders.rootId, rootId)).all()
    return new Map(rows.map((row) => [row.relDir, row.mtime]))
  }

  /** Stores listed folders; `replace` drops all other folders of the root (after a full scan). */
  saveFolders(rootId: number, listed: Map<string, number>, { replace = false } = {}): void {
    const rows = [...listed].map(([relDir, mtime]) => ({ rootId, relDir, mtime }))
    this.#db.transaction((tx) => {
      if (replace) tx.delete(folders).where(eq(folders.rootId, rootId)).run()
      for (let i = 0; i < rows.length; i += INSERT_CHUNK) {
        tx.insert(folders)
          .values(rows.slice(i, i + INSERT_CHUNK))
          .onConflictDoUpdate({ target: [folders.rootId, folders.relDir], set: { mtime: sql`excluded.mtime` } })
          .run()
      }
    })
  }

  /** Removes vanished folders with everything below them: subfolders and media. */
  removeFolders(rootId: number, relDirs: string[]): void {
    this.#db.transaction((tx) => {
      for (const relDir of relDirs) {
        tx.delete(media)
          .where(and(eq(media.rootId, rootId), subfolderRange(media.dir, relDir)))
          .run()
        tx.delete(folders)
          .where(
            and(eq(folders.rootId, rootId), or(eq(folders.relDir, relDir), subfolderRange(folders.relDir, relDir))),
          )
          .run()
      }
    })
  }

  #existing(where: SQL | undefined): Map<string, ExistingEntry> {
    const rows = this.#db
      .select({
        id: media.id,
        relPath: media.relPath,
        size: media.size,
        mtime: media.mtime,
        sidecarMtime: media.sidecarMtime,
        metaVersion: media.metaVersion,
      })
      .from(media)
      .where(where)
      .all()
    return new Map(rows.map(({ relPath, ...entry }) => [relPath, entry]))
  }

  /** Upserts files and replaces their tags in one transaction. */
  write(rootId: number, files: IndexedFile[]): void {
    try {
      this.#db.transaction((tx) => {
        for (const file of files) this.#writeFile(tx, rootId, file)
      })
    } catch (error) {
      this.#clearCaches()
      throw error
    }
  }

  remove(ids: number[]): void {
    this.#db.transaction((tx) => {
      for (let i = 0; i < ids.length; i += DELETE_CHUNK) {
        tx.delete(media)
          .where(inArray(media.id, ids.slice(i, i + DELETE_CHUNK)))
          .run()
      }
    })
  }

  /** Deletes tags without media or child tags, bottom-up. */
  pruneTags(): void {
    this.#clearCaches()
    const prune = sql`DELETE FROM ${tags} WHERE NOT EXISTS (SELECT 1 FROM ${mediaTags} WHERE ${mediaTags.tagId} = ${tags.id})
      AND NOT EXISTS (SELECT 1 FROM ${tags} AS child WHERE child.parent_id = ${tags.id})`
    let deleted: number
    do deleted = this.#db.run(prune).changes
    while (deleted > 0)
  }

  thumbSource(id: number): ThumbSource | undefined {
    return this.#db
      .select({
        id: media.id,
        rootId: media.rootId,
        rootPath: libraryRoots.path,
        relPath: media.relPath,
        kind: media.kind,
        size: media.size,
        mtime: media.mtime,
        duration: media.duration,
      })
      .from(media)
      .innerJoin(libraryRoots, eq(libraryRoots.id, media.rootId))
      .where(eq(media.id, id))
      .get()
  }

  /** Newest media still waiting for a thumbnail, skipping offline roots and `exclude`. */
  pendingThumbs(limit: number, exclude: number[]): number[] {
    return this.#db
      .select({ id: media.id })
      .from(media)
      .innerJoin(libraryRoots, eq(libraryRoots.id, media.rootId))
      .where(
        and(
          eq(media.thumbStatus, 'pending'),
          ne(libraryRoots.status, 'offline'),
          exclude.length > 0 ? notInArray(media.id, exclude) : undefined,
        ),
      )
      .orderBy(desc(media.sortDate))
      .limit(limit)
      .all()
      .map((row) => row.id)
  }

  /** Stores the outcome unless the file changed while its thumbnail was rendered. `null` marks a failure. */
  setThumbnail({ id, size, mtime }: ThumbSource, thumbhash: string | null): void {
    this.#db
      .update(media)
      .set({ thumbhash, thumbStatus: thumbhash === null ? 'error' : 'ready' })
      .where(and(eq(media.id, id), eq(media.size, size), eq(media.mtime, mtime)))
      .run()
  }

  #writeFile(tx: Tx, rootId: number, { metadata, ...file }: IndexedFile): void {
    const { tags: tagPaths, ...columns } = metadata
    // Thumbnails only go stale when the file itself changed, not its sidecar.
    const contentChanged = sql`${media.size} <> excluded.size OR ${media.mtime} <> excluded.mtime`
    const row = tx
      .insert(media)
      .values({ rootId, ...file, ...columns, metaVersion: METADATA_VERSION })
      .onConflictDoUpdate({
        target: [media.rootId, media.relPath],
        set: {
          fileName: sql`excluded.file_name`,
          kind: sql`excluded.kind`,
          size: sql`excluded.size`,
          mtime: sql`excluded.mtime`,
          sidecarMtime: sql`excluded.sidecar_mtime`,
          width: sql`excluded.width`,
          height: sql`excluded.height`,
          duration: sql`excluded.duration`,
          takenAt: sql`excluded.taken_at`,
          rating: sql`excluded.rating`,
          metaVersion: sql`excluded.meta_version`,
          thumbhash: sql`CASE WHEN ${contentChanged} THEN NULL ELSE ${media.thumbhash} END`,
          thumbStatus: sql`CASE WHEN ${contentChanged} THEN 'pending' ELSE ${media.thumbStatus} END`,
        },
      })
      .returning({ id: media.id })
      .get()

    tx.delete(mediaTags).where(eq(mediaTags.mediaId, row.id)).run()
    const tagIds = new Set(tagPaths.map((path) => this.#tagPathId(tx, path)))
    if (tagIds.size > 0) {
      tx.insert(mediaTags)
        .values([...tagIds].map((tagId) => ({ mediaId: row.id, tagId })))
        .run()
    }
  }

  /** Resolves a tag path to the id of its leaf, creating missing tags and parent links. */
  #tagPathId(tx: Tx, path: string[]): number {
    let parentId: number | null = null
    let id: number | null = null
    for (const name of path) {
      id = this.#tagId(tx, name)
      if (parentId !== null) this.#linkParent(tx, id, parentId)
      parentId = id
    }
    if (id === null) throw new Error('Tag path must not be empty')
    return id
  }

  #tagId(tx: Tx, name: string): number {
    const nameNorm = normalizeTag(name)
    const cached = this.#tagIds.get(nameNorm)
    if (cached !== undefined) return cached

    tx.insert(tags)
      .values({ name: cleanTag(name), nameNorm })
      .onConflictDoNothing({ target: tags.nameNorm })
      .run()
    const row = tx.select({ id: tags.id, parentId: tags.parentId }).from(tags).where(eq(tags.nameNorm, nameNorm)).get()
    if (!row) throw new Error(`Tag "${nameNorm}" vanished after insert`)
    this.#tagIds.set(nameNorm, row.id)
    this.#parents.set(row.id, row.parentId)
    return row.id
  }

  /** Tag names are globally unique, so the first hierarchy seen for a tag wins. */
  #linkParent(tx: Tx, id: number, parentId: number): void {
    if (this.#parentOf(tx, id) !== null) return
    // Refuse links that would make the tag its own ancestor (e.g. `A|B` and `B|A`).
    for (let ancestor: number | null = parentId; ancestor !== null; ancestor = this.#parentOf(tx, ancestor)) {
      if (ancestor === id) return
    }
    tx.update(tags).set({ parentId }).where(eq(tags.id, id)).run()
    this.#parents.set(id, parentId)
  }

  #parentOf(tx: Tx, id: number): number | null {
    const cached = this.#parents.get(id)
    if (cached !== undefined) return cached
    const parentId = tx.select({ parentId: tags.parentId }).from(tags).where(eq(tags.id, id)).get()?.parentId ?? null
    this.#parents.set(id, parentId)
    return parentId
  }

  #clearCaches(): void {
    this.#tagIds.clear()
    this.#parents.clear()
  }
}
