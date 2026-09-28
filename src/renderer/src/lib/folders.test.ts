import { describe, expect, it } from 'vitest'
import { buildFolderTree, folderAncestors } from './folders'

describe('buildFolderTree', () => {
  it('adds intermediate folders, sums counts and sorts naturally', () => {
    const tree = buildFolderTree([
      { path: '2024/10', count: 2 },
      { path: '2024/9/party', count: 3 },
      { path: '2023', count: 1 },
    ])

    expect(tree.map((node) => [node.path, node.count])).toEqual([
      ['2023', 1],
      ['2024', 5],
    ])
    expect(tree[1]?.children.map((node) => [node.name, node.count])).toEqual([
      ['9', 3],
      ['10', 2],
    ])
    expect(tree[1]?.children[0]?.children[0]).toMatchObject({ path: '2024/9/party', count: 3, children: [] })
  })
})

describe('folderAncestors', () => {
  it('lists the path and its ancestors', () => {
    expect(folderAncestors('a/b/c')).toEqual(['a', 'a/b', 'a/b/c'])
  })
})
