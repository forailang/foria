# Hello World

This chapter walks through the simplest complete forai program: a pipeline that reads a line of user input and prints a greeting. Along the way it demonstrates the four constructs — source, func, flow, sink — and shows how to run the program with `forai run`.

## Project Layout

```
hello/
├── main.fa
├── sources/
│   └── Input.fa
├── greet/
│   └── Format.fa
└── sinks/
    └── Print.fa
```

Every file contains one callable. The entry point is `main.fa`. The other three files each implement one stage of the pipeline.

## The Source: Reading Input

The source waits for user input and emits each line as a text event. When the user types "quit", the source stops.

```fa
# sources/Input.fa

docs Input
    Reads lines from the terminal and emits each as a text event.
    Stops when the user types "quit".
done

source Input
    emit line as text
    fail error as text
body
    on :input from term.prompt("Enter name (or quit): ") to raw
        trimmed = str.trim(raw)
        case trimmed
            when "quit"
                break
        done
        emit trimmed
    done
done

test Input
    # Source: reads from terminal, integration tested separately.
    ok = true
    must ok == true
done
```

The `on :input from term.prompt(...) to raw` block is the event loop. Each time `term.prompt` returns a line, the body runs once with `raw` bound to that line. `break` exits the loop, stopping the source. `emit` sends the current line downstream.

## The Func: Building the Greeting

The func receives a name and returns a formatted greeting string. This is pure computation — no I/O.

```fa
# greet/Format.fa

docs Format
    Formats a greeting message for the given name.
done

func Format
    take name as text
    emit result as text
    fail error as text
body
    msg = "Hello, #{name}!"
    emit msg
done

test Format
    must Format("World") == "Hello, World!"
    must Format("forai") == "Hello, forai!"
done
```

String interpolation uses `#{}`. The expression inside the braces is evaluated and inserted into the string. The `emit result as text` port declaration names what this func produces; the `fail error as text` port names what it produces on the failure track (even if this particular func never fails).

## The Sink: Printing Output

The sink receives a line of text and prints it to the terminal. It is a side-effect-only endpoint — what the spec calls "terminal I/O."

```fa
# sinks/Print.fa

docs Print
    Prints a text line to the terminal.
done

sink Print
    take line as text
    emit done as bool
    fail error as text
body
    _ = term.print(line)
    ok = true
    emit ok
done

test Print
    r = Print("hello test")
    must r == true
done
```

Sinks look identical to funcs in syntax. The `sink` keyword is a semantic signal to the compiler and reader that this callable performs terminal I/O — it does not transform data for further use.

## The Flow: Wiring It Together

The flow declares the pipeline shape. It names no computation of its own — only which stages connect to which.

```fa
# main.fa

use sources from "./sources"
use greet from "./greet"
use sinks from "./sinks"

docs main
    Hello world pipeline.
    Reads a name from the terminal, formats a greeting, and prints it.
    Repeats until the user types "quit".
done

flow main
body
    step sources.Input() then
        next :line to name
    done
    step greet.Format(name to :name) then
        next :result to greeting
    done
    step sinks.Print(greeting to :line) done
done

test main
    mock sources.Input => "World"
    mock greet.Format => "Hello, World!"
    mock sinks.Print => true
    _ = main()
done
```

Reading the flow body from top to bottom tells you everything about the data path:

1. `sources.Input()` runs, its `:line` port feeds `name`.
2. `greet.Format(name to :name)` runs, its `:result` port feeds `greeting`.
3. `sinks.Print(greeting to :line)` runs, consuming the result.

The `to :portName` syntax in a step call maps a local variable to the named `take` port of the callee. `then / next :port to var / done` extracts the named `emit` port result into a local variable.

## Running the Program

From the `hello/` directory:

```sh
forai run main.fa
```

The runtime compiles all files reachable from `main.fa` through `use` declarations, then starts the pipeline. You will see the prompt and can type names until you enter "quit".

## Testing the Program

```sh
forai test .
forai test sources/
forai test greet/
forai test sinks/
```

The `test main` block in `main.fa` uses `mock` to substitute all three sub-callables with fixed values, so the test runs without terminal I/O. Each individual file's test block tests that callable in isolation.

## Key Takeaways

- Every stage lives in its own file, named to match the callable inside.
- Sources use `on :tag from op(args) to var` to receive events; `emit` passes them downstream; `break` stops the loop.
- Funcs use `take`/`emit`/`fail` ports and a `body...done` block for computation.
- Sinks are syntactically identical to funcs; the keyword signals terminal I/O intent.
- Flows only wire stages together. No computation, no I/O, no loops.
- `main` must be a flow.
