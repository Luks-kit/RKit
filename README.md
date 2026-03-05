# LKit Language Design Document

LKit is a statically typed, compiled language targeting native code via LLVM.
There are multiple implementations: **rkit** (Rust), **CKit** (C), and **PyKit** (Python).
Source files use the `.lk` extension.

---

## Type System

### Primitive Types
| LKit    | LLVM    | Copy? |
|---------|---------|-------|
| `int`   | `i64`   | yes   |
| `float` | `f64`   | yes   |
| `bool`  | `i1`    | yes   |
| `str`   | `ptr`   | no    |
| `void`  | —       | yes   |
| `ptr`   | `ptr`   | no    |
| `byte`  | `i8`    | yes   |

`ptr` is an opaque untyped pointer, used for C interop only. It is equivalent to `byte*`.

### Composite Types
- **Fixed slice** `T[N]` — stack allocated, `N * sizeof(T)` bytes, size is part of the type
- **Dynamic slice** `[T]` — heap allocated `{ ptr, i64 len, i64 cap }`, RAII freed at region end
- **Struct** — aggregate of named fields; copy if all fields are copy AND total size ≤ 8 bytes

### Copy vs Move
A type is **copy-default** if:
1. All members are copy-default, AND
2. The struct fits in ≤ 8 bytes

Otherwise it is **move-default** (original invalidated on assignment).

---

## Ownership & References

### Owner Types
- `T` — owner, stored directly (stack/region)
- `T*` — owner with heap indirection; RAII calls `T`'s `dinit` (if defined) then `free` at region end

### Handle Types
- `T&` — shared read handle (many allowed simultaneously), copy-default
- `T strict&` — exclusive write handle (no other handles may exist), move-default

### Handle Creation
```
int x = 10;
int& r = &x;                // shared read handle
int strict& w = &strict x;  // exclusive write handle
```

### Handle Rules
- A handle cannot outlive its referent's region
- While a `strict&` exists, no other handles to the same variable can exist
- While any `&` exists, no `strict&` to the same variable can exist
- Handles cannot be returned from functions (would outlive referent)
- Handles cannot be stored in struct fields (for now)
- Reads through handles auto-deref
- Writes through `strict&` auto-deref on assignment
- `T*` always auto-derefs on read and write (exclusively owned)
- Borrow state tracked per-scope and released on scope exit

### Borrow State Tracking
The type checker tracks `(shared_count, has_exclusive)` per variable.
- `&x` increments shared count; decremented when the handle's scope exits
- `&strict x` sets exclusive flag; cleared when the handle's scope exits
- Both checks happen at declaration time and reject invalid combinations

### Cast
```
cast(T, expr)   // explicit type cast — escape hatch for C interop
```
Supported casts: ptr↔ptr (no-op in LLVM opaque ptr), int↔ptr, int↔int, float↔int, int↔float.

---

## Region System

Each lexical scope `{}` is a **region node** in a tree. Variables in a region can only
access other variables in the same region or direct ancestor regions. No reaching into
sibling scopes. Heap memory (`T*`, `[T]`) is freed automatically when its region exits (RAII).

---

## Extend System

Behavior is added to types via `extend` blocks. This is the mechanism for:
- `init` — constructor, called when a value of the type is created
- `dinit` — destructor, called automatically when the value's region exits (RAII)
- Methods — functions that take `this` as first parameter

```
struct Counter {
    int value;
    int id;
}

extend Counter {
    init(int id, int start) {
        this.id = id;
        this.value = start;
    }

    dinit {
        // cleanup, e.g. free resources
    }

    fn void increment(Counter strict& this) {
        this.value = this.value + 1;
    }

    fn int get(Counter& this) {
        return this.value;
    }
}

// Usage:
Counter c = Counter(1, 0);  // init called
c.increment();               // dot call syntax
int v = c.get();
// dinit called automatically at region end
```

### Extend Rules
- `init` — `this` is implicit, no first param, return type inferred as the struct type
- `dinit` — `this` is implicit, no params, no return, always void
- Methods — first param must be `T&` or `T strict&`; determines mutability
- `init` is called via `TypeName(args...)` syntax
- Method calls use dot syntax: `value.method(args...)`
- Overloads not yet supported (planned)
- `T*` RAII is implicit — if `T` has a `dinit`, `T*` calls it + `free` at scope end

---

## Module System

Files are organized into modules. Each `.lk` file is a module.

```
import math;         // looks for math.lk in search path
math.add(1, 2);      // qualified access
```

### Module Rules
- Import syntax: `import name;`
- All access is qualified: `module.function(args)`
- No circular imports
- Search paths: current directory, `/usr/local/lib/lkit`
- Single compilation unit — all modules merged before type checking
- Module functions mangled as `module__function` in LLVM IR

### Compilation Model
```
main.lk + imported modules
  → each file lexed and parsed independently
  → modules registered in type checker (structs, externs, functions)
  → main file type checked
  → modules compiled first (mangled names)
  → main file compiled
  → single .o output
  → gcc/clang links to binary
```

---

## Syntax

### Variable Declaration
```
int x = 10;           // explicit type
float f = 3.14;
bool b = true;
let y = 10;           // type inference (inferred as int)
let z = 3.14;         // inferred as float
```
`let` is resolved to a typed `VarDecl` after the type checker pass.

### Functions
```
fn int add(int a, int b) {
    return a + b;
}

fn void greet() {
    printf("hello\n");
    return;
}
```

### Extern Declarations
```
extern fn int printf(str fmt, ...);
extern fn void exit(int code);
```
`...` marks a variadic function.

### Structs
```
struct Point {
    int x;
    int y;
}

// Named init (aggregate, no extend needed)
Point p = Point { x: 1, y: 2 };

// Positional init
Point p = Point { 1, 2 };

// Field access
int a = p.x;
p.y = 10;
```

### Heap Allocation
```
// manual allocation via malloc + cast
int* p = cast(int*, malloc(8));
p = 42;           // auto-deref write
int x = p;        // auto-deref read

// struct on heap
Point* pt = cast(Point*, malloc(16));
pt.x = 1;         // auto-deref field access
```
RAII: `dinit` + `free` called automatically at region end.

### Fixed Slices
```
int[5] arr = [1, 2, 3, 4, 5];
int x = arr[0];        // compile-time bounds check if index is constant
arr[2] = 99;           // index assignment
int n = len(arr);      // compile-time constant
```
- Constant index → bounds checked at compile time (type error if OOB)
- Variable index → runtime bounds check emitted (calls `abort` on failure)

### Dynamic Slices (planned)
```
[int] arr = [1, 2, 3];
arr += 4;              // append
arr--;                 // pop back
arr += [1](99);        // insert 99 at index 1
arr -= [1];            // delete at index 1
int x = arr[0];        // bounds checked read
arr[0] = 5;            // bounds checked write
int n = len(arr);      // runtime length
```
Memory layout: `{ ptr, i64 len, i64 cap }`. RAII freed at region end.
Dynamic slices are move-default. Use `[T]&` or `[T] strict&` for handles.

### Control Flow
```
if (condition) { ... } else { ... }
while (condition) { ... }
return expr;
return;         // void return
```

### Comments
```
// single line comment
/* block comment */
```

### Operators
| Category    | Operators                          |
|-------------|------------------------------------|
| Arithmetic  | `+` `-` `*` `/`                    |
| Comparison  | `==` `!=` `<` `<=` `>` `>=`       |
| Logical     | `&&` `\|\|`                        |
| Bitwise     | `&` `\|`                           |
| Unary       | `-` `!`                            |
| Handle      | `&x` `&strict x`                   |
| Variadic    | `...`                              |
| Cast        | `cast(T, expr)`                    |

---

## Pipeline

```
.lk source
  → Lexer        (tokens with line numbers)
  → Parser       (AST)
  → TypeChecker  (three-pass: structs, extends, functions registered; then full check)
  → Transform    (fold LetDecl → VarDecl, etc.)
  → Compiler     (LLVM IR via inkwell)
  → Object file  (.o)
  → gcc/clang    (link to binary)
```

### Type Checker Details
- Three-pass: structs registered, then extends, then function signatures, then full check
- Collects all errors rather than stopping at first
- Enforces strong static typing — no implicit coercions
- Numeric widening: byte+int→int, int+float→float, byte+float→float
- Int→Byte narrowing allowed in assignments and casts
- Checks handle exclusivity rules with scope-aware borrow state
- Constant slice index bounds checked at compile time
- `LetDecl` inferred and folded into `VarDecl` before codegen
- Borrow state stored as `(shared_count, has_exclusive)` per variable
- Scope stack stores `(LKitType, Option<String>)` — type and optional referent name
- Module exports registered separately; qualified calls resolved via `module_exports`

---

## Compiler (rkit) Details

- Built with inkwell targeting LLVM 18.1
- `VarSlot` stores `{ ptr, ty, is_ref, type_name }` for each variable
- Variables stored in a scope stack `Vec<HashMap<String, VarSlot>>` mirroring type checker
- Field access emits direct GEP + load (no temp alloca)
- Handles are pointers in LLVM; auto-deref on read via `type_name` stripping
- `T*` auto-derefs on read and write; pointee type resolved from `type_name`
- Struct types resolved by name via `struct_defs` registry
- Extend blocks compiled to mangled functions: `TypeName__init`, `TypeName__dinit`, `TypeName_method`
- Module functions mangled as `module__function`
- `dinit` emitted automatically at scope exit for any variable whose type has a dinit defined
- `T*` scope exit: calls dinit (if defined) then `free`
- `dinit` also emitted before every `return` statement for all active scopes
- `abort()` called on runtime OOB slice access
- LLVM module verified before object file emission
- Truncation emitted automatically when storing wider int into narrower pointer target

---

## Planned / Not Yet Implemented
- Dynamic slices `[T]` with RAII (next)
- Option types (the exception to non-null handles)
- Overloaded `init` methods
- `extend` for built-in types
- Warnings (unused variables etc.)
- Full region/lifetime enforcement beyond simple rules
- Chained field access `a.b.c`
- Complex lvalue expressions beyond variable/field/index
- `for` loops
- String operations
- Pointer arithmetic
