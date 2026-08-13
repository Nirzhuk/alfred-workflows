# Component structure

## Where components live

- **Generic / shared UI** → `src/components/<kebab-name>/`
- **Feature-specific UI** → `src/features/<feature>/components/<kebab-name>/`

## Naming

- **React export**: PascalCase (`AppTitleBar`)
- **Folder and files**: kebab-case (`app-title-bar`)

| Export          | Folder / files   |
|-----------------|------------------|
| `AppTitleBar`   | `app-title-bar`  |
| `ConfirmDialog` | `confirm-dialog` |
| `NodeInspector` | `node-inspector` |

## Layout (colocated tests)

Every component folder contains:

```
app-title-bar/
  app-title-bar.tsx        # implementation
  app-title-bar.test.tsx   # tests
  index.ts                 # public re-export
```

### Shared example

```
src/components/confirm-dialog/
  confirm-dialog.tsx
  confirm-dialog.test.tsx
  index.ts
```

### Feature example

```
src/features/workflow/components/app-title-bar/
  app-title-bar.tsx
  app-title-bar.test.tsx
  index.ts
```

## Rules

- Never put a lone PascalCase file at a components root (❌ `AppTitleBar.tsx`)
- Never put tests in a separate top-level `src/tests/` tree — always colocate
- File names are kebab-case; only the exported symbol is PascalCase
- Import via the folder: `import { AppTitleBar } from "@/features/workflow/components/app-title-bar"`

## Example

```tsx
// app-title-bar.tsx
export function AppTitleBar() {
  return <header>…</header>;
}
```

```ts
// index.ts
export { AppTitleBar } from "./app-title-bar";
```

```tsx
// app-title-bar.test.tsx
import { AppTitleBar } from "./app-title-bar";
```
