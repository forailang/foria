# Chapter 13.3: Background Jobs

Background jobs are long-running tasks that should not block the main server loop. In forai, the standard pattern is to use a database-backed job queue: the main server inserts job records, and a separate worker flow polls for them and processes each one. The worker is launched with `send nowait` so it runs independently of the server.

## The Core Pattern

```
server ──insert job record──→ jobs table
                                    ↑
worker ──poll + process──────────────┘
```

The server and worker do not communicate directly. The database is the coordination point. This approach is durable: if the worker crashes, the job record remains in the database and the worker can pick it up after restart.

## Launching the Worker

The worker is started from the main server flow using `send nowait`. Because `send nowait` spawns an isolated task, the worker cannot use the server's database connection — it opens its own:

```fa
# server/Start.fa
flow Start
    emit result as StartResult
    fail error as StartError
body
    state conn = db.open("app.db")
    step db.Migrate(conn to :conn) done

    # Launch background worker — does not block server startup
    send nowait workflow.RunJobLoop()

    # Continue to accept HTTP requests
    step sources.HTTPRequests(8080 to :port) then
        next :req to req
    done
    step handler.HandleRequest(conn to :conn, req to :req) done
done
```

`RunJobLoop` has no arguments (or takes a trivial argument) because it opens its own connection.

## Job Queue Schema

A minimal job queue table:

```fa
docs MigrateJobs
    Creates the jobs table for the background job queue.
done

func MigrateJobs
    take conn as db_conn
    emit result as bool
    fail error as text
body
    ok = db.exec(conn, "CREATE TABLE IF NOT EXISTS jobs (
        id TEXT PRIMARY KEY,
        type TEXT NOT NULL,
        payload TEXT NOT NULL,
        status TEXT DEFAULT 'queued',
        attempt INTEGER DEFAULT 0,
        max_attempts INTEGER DEFAULT 3,
        created_at TEXT DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT DEFAULT CURRENT_TIMESTAMP
    )")
    emit true to :result
done
```

## Enqueuing a Job

Any func can insert a job record. The server does not need to do any extra work:

```fa
docs EnqueueEmailJob
    Enqueues an email-sending job for the background worker.
done

func EnqueueEmailJob
    take conn as db_conn
    take to_email as text
    take subject as text
    take body_text as text
    emit result as text
    fail error as text
body
    id = random.uuid()
    payload_obj = obj.new()
    payload_obj = obj.set(payload_obj, "to", to_email)
    payload_obj = obj.set(payload_obj, "subject", subject)
    payload_obj = obj.set(payload_obj, "body", body_text)
    payload_json = json.encode(payload_obj)

    params = list.new()
    params = list.append(params, id)
    params = list.append(params, "send_email")
    params = list.append(params, payload_json)
    ok = db.exec(conn, "INSERT INTO jobs (id, type, payload) VALUES (?1, ?2, ?3)", params)
    emit id to :result
done
```

## The Worker Loop (source-based)

The worker uses a `source` construct with a `loop` to poll for queued jobs. Between polls, it sleeps to avoid burning CPU:

```fa
# sources/QueuedJobs.fa
docs QueuedJobs
    Polls the database for queued jobs and emits them one at a time.
    Sleeps 2 seconds between polls when no jobs are found.
done

source QueuedJobs
    take conn as db_conn
    emit job as dict
    fail error as text
body
    loop list.range(0, 999999) as _
        rows = db.query(conn, "SELECT id, type, payload, attempt FROM jobs WHERE status = 'queued' ORDER BY created_at ASC LIMIT 10")
        count = list.len(rows)
        case count
            when 0
                time.sleep(2000)
            else
                loop rows as row
                    emit row
                done
        done
    done
done
```

## The Worker Flow

The flow wires the source to the processing func:

```fa
# workflow/RunJobLoop.fa
use sources from "./sources"
use jobs from "./jobs"

docs RunJobLoop
    Background job runner. Opens its own DB connection, polls for queued
    jobs, and drives them through the state machine. Launched as a
    fire-and-forget task from the server.
done

flow RunJobLoop
    emit result as RunJobLoopResult
    fail error as RunJobLoopError
body
    state conn = db.open("app.db")
    step sources.QueuedJobs(conn to :conn) then
        next :job to job
    done
    step jobs.ProcessJob(conn to :conn, job to :job) done
done
```

## Processing a Job

The job processor marks the job as in-progress, does the work, then marks it done or failed:

```fa
# jobs/ProcessJob.fa
docs ProcessJob
    Processes a single job record. Marks it in-progress, executes the
    appropriate handler, then marks it done or failed.
done

func ProcessJob
    take conn as db_conn
    take job as dict
    emit result as bool
    fail error as text
body
    job_id = obj.get(job, "id")
    job_type = obj.get(job, "type")
    payload_json = obj.get(job, "payload")
    attempt = obj.get(job, "attempt")
    new_attempt = attempt + 1

    # Mark as in-progress
    params_start = list.new()
    params_start = list.append(params_start, new_attempt)
    params_start = list.append(params_start, job_id)
    ok = db.exec(conn, "UPDATE jobs SET status = 'running', attempt = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2", params_start)

    payload = json.decode(payload_json)

    # Dispatch by job type
    success = false
    case job_type
        when "send_email"
            success = jobs.SendEmailJob(payload to :payload)
        when "resize_image"
            success = jobs.ResizeImageJob(payload to :payload)
        else
            success = false
    done

    # Mark as done or failed
    final_status = "done"
    case success
        when false
            final_status = "failed"
        else
    done

    params_end = list.new()
    params_end = list.append(params_end, final_status)
    params_end = list.append(params_end, job_id)
    ok = db.exec(conn, "UPDATE jobs SET status = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2", params_end)

    emit true to :result
done
```

## Retry Logic

To retry failed jobs, the worker can re-queue them if `attempt < max_attempts`:

```fa
func MaybeRetryJob
    take conn as db_conn
    take job_id as text
    take attempt as long
    take max_attempts as long
    emit result as bool
    fail error as text
body
    remaining = max_attempts - attempt
    new_status = "failed"
    case remaining
        when 0
        else
            new_status = "queued"
    done
    params = list.new()
    params = list.append(params, new_status)
    params = list.append(params, job_id)
    ok = db.exec(conn, "UPDATE jobs SET status = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2", params)
    emit true to :result
done
```

## Polling Delay with time.sleep

`time.sleep(ms)` pauses execution for the given number of milliseconds. Use it inside the source loop to avoid busy-polling:

```fa
# Sleep 5 seconds between polls
time.sleep(5000)
```

Common polling intervals:
- `1000` — 1 second (low-latency jobs)
- `5000` — 5 seconds (normal jobs)
- `30000` — 30 seconds (infrequent batch jobs)

## Why Not use send nowait for Each Job?

It is tempting to `send nowait ProcessJob(...)` for each job so they run in parallel. This works, but it creates resource pressure: if 1000 jobs are queued, all 1000 would be spawned simultaneously, each opening its own database connection and consuming memory.

The loop-based approach naturally limits concurrency: the source emits one job, the worker processes it, then the source emits the next. For more parallelism, use a `sync` block to process a small batch at a time:

```fa
# Process up to 5 jobs concurrently per poll cycle
# (simplified — real implementation would use dynamic sync)
[r1, r2, r3, r4, r5] = sync :timeout => 30s, :safe => true
    r1 = jobs.ProcessJob(conn to :conn, batch[0] to :job)
    r2 = jobs.ProcessJob(conn to :conn, batch[1] to :job)
    r3 = jobs.ProcessJob(conn to :conn, batch[2] to :job)
    r4 = jobs.ProcessJob(conn to :conn, batch[3] to :job)
    r5 = jobs.ProcessJob(conn to :conn, batch[4] to :job)
done [r1, r2, r3, r4, r5]
```
