export interface FolderNode {
  name: string
  /** `/`-separated, relative to the library root. */
  path: string
  /** Media in this folder and all subfolders. */
  count: number
  children: FolderNode[]
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' })

/** Builds the folder tree from folders that directly contain media, adding the folders in between. */
export function buildFolderTree(folders: { path: string; count: number }[]): FolderNode[] {
  const root: FolderNode = { name: '', path: '', count: 0, children: [] }
  const nodes = new Map<string, FolderNode>([['', root]])

  for (const { path, count } of folders) {
    let parent = root
    let current = ''
    for (const name of path.split('/')) {
      current = current ? `${current}/${name}` : name
      let node = nodes.get(current)
      if (!node) {
        node = { name, path: current, count: 0, children: [] }
        nodes.set(current, node)
        parent.children.push(node)
      }
      node.count += count
      parent = node
    }
  }

  for (const node of nodes.values()) node.children.sort((a, b) => collator.compare(a.name, b.name))
  return root.children
}

/** `path` and all its ancestors, e.g. `a`, `a/b` for `a/b`. */
export function folderAncestors(path: string): string[] {
  const segments = path.split('/')
  return segments.map((_, i) => segments.slice(0, i + 1).join('/'))
}
