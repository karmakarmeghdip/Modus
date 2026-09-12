# Modus Multi-Module Examples

This directory demonstrates Modus's ECMAScript-style module system, incremental compilation, and shared library dynamic linking.

## Files

1. **`math.mds`**: Pure mathematical utility module exporting functions (`square`, `sum_range`, `factorial`).
2. **`geom.mds`**: Geometry module demonstrating named imports from `math.mds` (`import { square } from "./math.mds"`) and exporting records and functions (`Point`, `distance_sq`, `manhattan`).
3. **`utils.mds`**: Utility module demonstrating side-effect imports (`import "./utils.mds"`).
4. **`main.mds`**: Application entry point demonstrating:
   - Named imports: `import { sum_range, factorial } from "./math.mds";`
   - Namespace imports: `import * as Geo from "./geom.mds";`
   - Side-effect imports: `import "./utils.mds";`
5. **`calc_lib.mds`**: Library source file designed to be precompiled into a shared library (`.so`).
6. **`app_dyn.mds`**: Consumer application that imports precompiled shared library prototypes via an export map header (`library "./libcalc.so";`).

---

## Running and Building

### 1. JIT Execution
Execute multi-module source files directly via Inkwell JIT:
```bash
modus run examples/modules/main.mds
```

### 2. Standalone Native Executable
Compile to a standalone native binary with incremental caching:
```bash
modus build examples/modules/main.mds -o examples/modules/app
./examples/modules/app
```

### 3. Precompiled Shared Library (`.so`) & Export Map Header
Compile a shared library and automatically generate its `.mds` export map header:
```bash
# Compile shared library and emit header
modus build --lib examples/modules/calc_lib.mds -o examples/modules/libcalc.so --emit-header examples/modules/calc.mds

# JIT execute or compile consumer application dynamically linking against libcalc.so
modus run examples/modules/app_dyn.mds
modus build examples/modules/app_dyn.mds -o examples/modules/app_dyn
./examples/modules/app_dyn
```
