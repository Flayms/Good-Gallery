import { describe, expect, it } from 'vitest'
import { buildTagTree } from './tag-tree'

describe('buildTagTree', () => {
  it('nests tags under their parent and keeps unparented tags at the top level', () => {
    const tree = buildTagTree([
      { id: 1, name: 'Anton', parentId: null, count: 3 },
      { id: 2, name: 'Bea', parentId: null, count: 1 },
    ])

    expect(tree.map((node) => [node.name, node.count, node.children])).toEqual([
      ['Anton', 3, []],
      ['Bea', 1, []],
    ])
  })

  it('builds multiple levels from flat parentId links', () => {
    const tree = buildTagTree([
      { id: 1, name: 'France', parentId: null, count: 2 },
      { id: 2, name: 'Paris', parentId: 1, count: 2 },
    ])

    expect(tree).toEqual([
      { id: 1, name: 'France', count: 2, children: [{ id: 2, name: 'Paris', count: 2, children: [] }] },
    ])
  })
})
