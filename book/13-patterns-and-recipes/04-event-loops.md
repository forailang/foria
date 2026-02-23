# Chapter 13.4: Event Loops

An event loop is a program that continuously waits for and responds to incoming events — user input, network messages, timer ticks, or queue entries. In forai, event loops are built with the `source` construct, which wraps a blocking wait in a repeatable pattern. This chapter shows how to build event loops for terminal interaction, WebSocket streams, and custom polling.

## The source Construct

A `source` is an event producer. Its body runs a loop that calls a blocking operation, receives an event, optionally processes it, and emits it downstream. The flow receiving from the source only sees the stream of emitted events — it never sees the waiting.

The key body pattern is `on :tag from op(args) to var`:

```fa
source MySource
    emit event as dict
    fail error as text
body
    on :input from some.blocking_op(args) to raw
        processed = transform(raw)
        emit processed
    done
done
```

The `on` block runs once per event. The source continues blocking and emitting until `break` is called or the program exits.

## Terminal REPL

A terminal REPL (read-eval-print loop) reads lines from the user and emits them as events:

```fa
# sources/Commands.fa
docs Commands
    Reads lines from the terminal and emits them as command strings.
    Emits "quit" and "exit" as regular commands before stopping.
done

source Commands
    emit cmd as text
    fail error as text
body
    on :input from term.prompt(">> ") to raw
        trimmed = str.trim(raw)
        emit trimmed
        case trimmed
            when "quit"
                break
            when "exit"
                break
        done
    done
done
```

The `break` inside the `on` block stops the source loop. The flow sees a regular end-of-stream.

The flow processes each command:

```fa
# app/Start.fa
use sources from "./sources"
use handler from "./handler"

docs Start
    Terminal REPL: read commands, dispatch to handlers.
done

flow Start
    emit result as dict
    fail error as dict
body
    step sources.Commands() then
        next :cmd to cmd
    done
    step handler.Dispatch(cmd to :cmd) then
        next :result to response
    done
    step Display(response to :text) done
done
```

And the dispatch func routes by command:

```fa
docs Dispatch
    Routes a command string to the appropriate handler.
done

func Dispatch
    take cmd as text
    emit result as text
    fail error as text
body
    parts = str.split(cmd, " ")
    verb = parts[0]

    result = ""
    case verb
        when "help"
            result = "Commands: help, ls, cat <file>, quit"
        when "ls"
            files = file.list(".")
            result = str.join(files, "\n")
        when "cat"
            filename = parts[1]
            content = file.read(filename)
            result = content
        when "quit"
            result = "Goodbye!"
        else
            result = "Unknown command: " + cmd
    done
    emit result to :result
done
```

## WebSocket Event Stream

A WebSocket source emits messages received from a server:

```fa
# sources/WSMessages.fa
docs WSMessages
    Connects to a WebSocket server and emits incoming messages.
done

source WSMessages
    take url as text
    emit message as dict
    fail error as text
body
    ws_conn = ws.connect(url)
    on :message from ws.recv(ws_conn) to raw_msg
        parsed = json.decode(raw_msg)
        msg_type = obj.get(parsed, "type")
        case msg_type
            when "ping"
                ws.send(ws_conn, "{\"type\": \"pong\"}")
            else
                emit parsed
        done
    done
    ws.close(ws_conn)
done
```

This source:
- Connects once at startup.
- Receives messages one at a time via `ws.recv`.
- Filters out ping messages (responds with pong, does not emit downstream).
- Emits all other messages as parsed dicts.
- Closes the connection when the `on` block exits (either `break` or connection closed by server).

Using the WebSocket source in a flow:

```fa
flow WatchPrices
    emit result as dict
    fail error as dict
body
    step sources.WSMessages("wss://prices.example.com/stream" to :url) then
        next :message to msg
    done
    step handler.ProcessPriceUpdate(msg to :msg) done
done
```

## Polling Loop (timer-based)

For polling an external API or database on a schedule, use a `loop` inside the source body with `time.sleep`:

```fa
# sources/PollFeed.fa
docs PollFeed
    Polls an RSS feed URL every 60 seconds and emits new items.
done

source PollFeed
    take feed_url as text
    emit item as dict
    fail error as text
body
    seen_ids = list.new()
    loop list.range(0, 999999) as _
        response = http.get(feed_url)
        items = obj.get(response, "items")
        loop items as item
            item_id = obj.get(item, "id")
            is_seen = list.contains(seen_ids, item_id)
            case is_seen
                when false
                    seen_ids = list.append(seen_ids, item_id)
                    emit item
                else
            done
        done
        time.sleep(60000)
    done
done
```

## HTTP Long-Polling

A source that polls an HTTP endpoint for new events and emits them:

```fa
# sources/LongPoll.fa
docs LongPoll
    Polls an HTTP endpoint every 2 seconds for new events.
    Emits each event as a dict.
done

source LongPoll
    take endpoint as text
    take since_id as text
    emit event as dict
    fail error as text
body
    cursor = since_id
    loop list.range(0, 999999) as _
        params_url = endpoint + "?since=" + cursor
        response = http.get(params_url)
        events = obj.get(response, "events")
        count = list.len(events)
        case count
            when 0
                time.sleep(2000)
            else
                loop events as event
                    event_id = obj.get(event, "id")
                    cursor = event_id
                    emit event
                done
        done
    done
done
```

## Keyboard Event Loop

For interactive terminal UIs, read individual key presses:

```fa
# sources/KeyPresses.fa
docs KeyPresses
    Reads individual key presses from the terminal and emits them.
    Emits "q" to signal quit.
done

source KeyPresses
    emit key as text
    fail error as text
body
    on :key from term.read_key() to raw
        emit raw
        case raw
            when "q"
                break
            when "Q"
                break
        done
    done
done
```

## Combining Multiple Events in One Source

A source can emit from multiple event kinds using separate `on` blocks or conditional logic. Each `emit` sends one event downstream:

```fa
# sources/MultiInput.fa
docs MultiInput
    Reads from both stdin and a WebSocket, emitting all events tagged by source.
done

source MultiInput
    take ws_url as text
    emit event as dict
    fail error as text
body
    ws_conn = ws.connect(ws_url)
    # In practice, you'd use sync to wait on both simultaneously
    # This simplified version polls stdin then ws alternately
    loop list.range(0, 999999) as _
        line = term.prompt("")
        trimmed = str.trim(line)
        case trimmed
            when ""
            else
                ev = obj.new()
                ev = obj.set(ev, "source", "stdin")
                ev = obj.set(ev, "data", trimmed)
                emit ev
        done

        msg = ws.recv(ws_conn)
        case msg
            when ""
            else
                parsed = json.decode(msg)
                ev = obj.new()
                ev = obj.set(ev, "source", "ws")
                ev = obj.set(ev, "data", parsed)
                emit ev
        done
    done
done
```

## break to Stop the Loop

Inside an `on` block or a `loop` inside a source body, `break` terminates the source loop. The downstream flow sees the end of the stream and the pipeline finishes:

```fa
source BoundedStream
    take max_items as long
    emit item as long
    fail error as text
body
    count = 0
    on :tick from timer.tick(100) to _
        count = count + 1
        emit count
        case count
            when max_items
                break
            else
        done
    done
done
```

After `break`, the source exits and the pipeline drains any in-flight events.
