/**
 * Maps an LSP semantic token type (plus modifiers) to the CSS class used
 * for syntax highlighting in the editor. Returns null for unknown types.
 *
 * Lean 4's LSP provides only three token types — `keyword`, `variable`,
 * and `property` — with a `declaration` modifier to mark definition
 * sites.  The modifier is essential: without it, theorem names, let
 * bindings, and hypothesis names would be indistinguishable from plain
 * variable references.
 */
export function tokenTypeToCssClass(
  tokenType: string,
  modifiers: readonly string[] = [],
): string | null {
  const isDeclaration = modifiers.includes('declaration')

  switch (tokenType) {
    case 'keyword':
    case 'modifier':
      return 'cm-lean-keyword'
    case 'type':
    case 'class':
    case 'struct':
    case 'enum':
    case 'interface':
    case 'typeParameter':
      return 'cm-lean-type'
    case 'function':
    case 'method':
      return 'cm-lean-function'
    case 'variable':
    case 'parameter':
      return isDeclaration ? 'cm-lean-function' : 'cm-lean-variable'
    case 'property':
      return 'cm-lean-property'
    case 'namespace':
      return 'cm-lean-namespace'
    case 'enumMember':
      return 'cm-lean-enum-member'
    case 'macro':
      return 'cm-lean-macro'
    case 'comment':
      return 'cm-lean-comment'
    case 'string':
      return 'cm-lean-string'
    case 'number':
      return 'cm-lean-number'
    case 'operator':
      return 'cm-lean-operator'
    case 'decorator':
      return 'cm-lean-decorator'
    default:
      return null
  }
}
