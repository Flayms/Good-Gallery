import Database from 'better-sqlite3'
import { drizzle } from 'drizzle-orm/better-sqlite3'
import { migrate } from 'drizzle-orm/better-sqlite3/migrator'
import * as schema from './schema'

/** Pass `migrationsFolder` only from the process that owns the schema (main). */
export function openDatabase(file: string, migrationsFolder?: string) {
  const sqlite = new Database(file)
  sqlite.pragma('journal_mode = WAL')
  sqlite.pragma('synchronous = NORMAL')
  sqlite.pragma('foreign_keys = ON')
  sqlite.pragma('busy_timeout = 5000')

  const db = drizzle({ client: sqlite, schema })
  if (migrationsFolder) migrate(db, { migrationsFolder })
  return db
}

export type Db = ReturnType<typeof openDatabase>
