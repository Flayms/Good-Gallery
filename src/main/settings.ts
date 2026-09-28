import { EventEmitter } from 'node:events'
import { DEFAULT_SETTINGS, type Settings, settingsSchema } from '@shared/settings'
import type { Db } from './db'
import { settings as settingsTable } from './db/schema'

function isSettingsKey(key: string): key is keyof Settings {
  return Object.hasOwn(DEFAULT_SETTINGS, key)
}

/** Persisted user settings. Stored values that no longer validate fall back to their defaults. */
export class SettingsStore extends EventEmitter<{ change: [next: Settings, previous: Settings] }> {
  readonly #db: Db
  #current: Settings

  constructor(db: Db) {
    super()
    this.#db = db
    const current = { ...DEFAULT_SETTINGS }
    for (const { key, value } of db.select().from(settingsTable).all()) {
      if (!isSettingsKey(key)) continue
      const parsed = settingsSchema.shape[key].safeParse(value)
      if (parsed.success) Object.assign(current, { [key]: parsed.data })
    }
    this.#current = current
  }

  get(): Settings {
    return this.#current
  }

  update(patch: Partial<Settings>): Settings {
    const previous = this.#current
    const next = settingsSchema.parse({ ...previous, ...patch })
    this.#db.transaction((tx) => {
      for (const [key, value] of Object.entries(next)) {
        tx.insert(settingsTable)
          .values({ key, value })
          .onConflictDoUpdate({ target: settingsTable.key, set: { value } })
          .run()
      }
    })
    this.#current = next
    this.emit('change', next, previous)
    return next
  }
}
