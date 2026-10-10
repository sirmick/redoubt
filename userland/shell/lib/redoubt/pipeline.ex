defmodule Redoubt.Pipeline do
  @moduledoc """
  Pipelines of native stages (docs/userland/native.md, "Standard input and output, and pipes";
  docs/userland/shell.md, "Native programs and pipes").

  Each stage is a program from `/boot`, in a budget of its own carved from the session's, with
  exactly three namespace entries: `/dev/stdin`, `/dev/stdout` and `/dev/stderr`, each a
  connection the session's `piped` minted at one end of one pipe (`Redoubt.Pipes`). Nothing is
  inherited: a stage gets no other handle, and never the console.

  - **Joining.** Stage i's standard output is pipe i, the next stage's standard input. Pipe 0 is
    what the session feeds the first stage, and the last pipe what it reads from the last; every
    stage's standard error is one more pipe, which the session draws on the console.
  - **The ends.** When a stage ends, the session lets its connections go: its reader reads the end
    of the stream, and its writer's next write is refused. **The job is complete when its last
    stage ends**: nothing more can reach its output, so every other stage's budget is destroyed,
    and every budget of the job is destroyed before `run/2` returns.
  - **The owner.** One process owns the job: it launches the stages, so their exit notices come
    to it, and it watches the process that asked. If that process ends first (its line was
    interrupted, or crashed), the owner destroys every stage's budget and lets everything go, so
    no stage outlives the line that ran it. The owner is the job the session lists
    (`Redoubt.Jobs`), and killing the job is asking it to destroy every stage's budget now.
  - **In the background** (`start/2`, which `Redoubt.Job.start/2` calls), the owner watches no
    line: the job runs until it ends or is killed, reads no console, and keeps what it writes,
    bounded, for `Redoubt.Job.await/2`.
  - **What reaches the console** passes the shell's guard (docs/userland/shell.md, "Hostile text
    never drives the terminal"): a stage's output and standard error are written as the line's
    own output, so through the shell's driver and `Redoubt.Term`.
  """

  # beamlet's natives (docs/userland/beamlet.md, "Natives"): no module, so nothing to check at compile time.
  @compile {:no_warn_undefined, :redoubt}

  alias Redoubt.{Budget, Jobs, Pipes}
  alias Redoubt.Wire.Client.NinepCommon

  @default_budget %{pages: 256, processes: 1, weight: 1}
  # A session holds 10 processes: the VM, its console's relay, piped and 7 stages; piped lets the
  # session and its stages park 8 calls, a stage's one and the session's completion call
  # (docs/servers/piped.md). The VM runs 16 jobs at once (beamlet's MAX_JOBS), so it never binds.
  @max_stages 7
  # How much one read of a stage's output asks for.
  @read_bytes 4096
  # How long the stages destroyed at the job's end are given to send their exit notices.
  @notice_wait_ms 5_000
  # What a background job keeps of its output, and of its standard error, each.
  @kept_bytes 64 * 1024

  @typedoc "How a stage ended: `{:exited, code}`, `{:faulted, cause}` or `{:killed, 0}`."
  @type ending :: {:exited | :faulted | :killed | :ended, non_neg_integer()}

  @typedoc "A stage: a program's name in `/boot` and its arguments."
  @type stage :: {String.t(), [String.t()]}

  @doc "The most stages a pipeline has."
  @spec max_stages() :: pos_integer()
  def max_stages, do: @max_stages

  @doc """
  Splits `words` at each `"|"` into stages: `~w(grep -n x | wc -l)` is
  `[{"grep", ["-n", "x"]}, {"wc", ["-l"]}]`. An empty stage is `{:error, :empty_stage}`.
  """
  @spec stages([String.t()]) :: {:ok, [stage()]} | {:error, :empty_stage}
  def stages(words) do
    chunks =
      words
      |> Enum.reduce([[]], fn
        "|", chunks -> [[] | chunks]
        word, [chunk | chunks] -> [[word | chunk] | chunks]
      end)
      |> Enum.reverse()
      |> Enum.map(&Enum.reverse/1)

    if [] in chunks,
      do: {:error, :empty_stage},
      else: {:ok, Enum.map(chunks, fn [name | args] -> {name, args} end)}
  end

  @doc """
  Runs `stages` and waits for the last to end. Options:
  - `:input`: what the first stage reads: `nil` (the default) for nothing, the end of its input at
    once; `:console` for the lines the person types, until Ctrl+D; an enumerable of lines or
    binaries, each line written with a newline after it; or `{:records, host}`, what `host` hands
    the feeder (below).
  - `:output`: `:capture` (the default) to return what the last stage writes, `:console` to draw
    it as it comes, or `{:records, host}` to hand it to `host` a read at a time (below); then the
    stages' standard error is kept, as a background job's is, not drawn.
  - `:budget`: each stage's budget spec, `#{inspect(@default_budget)}` when left out.

  With `{:records, host}` (`Redoubt.Screen.Native`, a native program's screen), `host` is told
  `{#{inspect(__MODULE__)}, :stdin, feeder}` and sends the feeder `{:write, bytes}`, each answered
  `{#{inspect(__MODULE__)}, :written, n}` once written; and it is sent each read of the last
  stage's output as `{#{inspect(__MODULE__)}, :stdout, reader, bytes}`, the next read waiting for
  its `{#{inspect(__MODULE__)}, :more}`, so `host` paces the stage's writes. Either ends if `host`
  does.

  Returns `{:ok, %{output: bytes | nil, errors: nil, killed: bool, endings: [ending], usage:
  [usage]}}`, a stage's usage being what its budget held as it ended and `killed` whether the job
  was killed (`Redoubt.Job.kill/1`), or `{:error, name}` when it could not start. The job is the
  session's (`Redoubt.Jobs`) while it runs.
  """
  @spec run([stage()], keyword()) ::
          {:ok, %{output: binary() | nil, endings: [ending()], usage: [map() | nil]}} | {:error, atom()}
  def run(stages, opts \\ []) do
    caller = self()
    ref = make_ref()
    leader = Process.group_leader()

    {owner, monitor} =
      spawn_monitor(fn ->
        Process.group_leader(self(), leader)
        send(caller, {__MODULE__, ref, own(caller, stages, opts)})
      end)

    receive do
      {__MODULE__, ^ref, result} ->
        Process.demonitor(monitor, [:flush])
        result

      {:DOWN, ^monitor, :process, ^owner, reason} ->
        {:error, if(reason == :normal, do: :ended, else: :crashed)}
    end
  end

  @doc """
  Starts `stages` in the background and returns once every stage is launched: `{:ok, job}`
  (`Redoubt.Job`), or `{:error, name}` as `run/2`. Its first stage reads `:input`, lines, or
  nothing; what the last stage writes and every stage's standard error are kept, up to
  #{@kept_bytes} bytes each, for `Redoubt.Job.await/2`.
  """
  @spec start([stage()], keyword()) :: {:ok, Redoubt.Job.t()} | {:error, atom()}
  def start(stages, opts \\ []) do
    starter = self()
    ref = make_ref()
    leader = Process.group_leader()
    opts = Keyword.merge(opts, started: {starter, ref}, output: {:kept, @kept_bytes})

    {owner, monitor} =
      spawn_monitor(fn ->
        Process.group_leader(self(), leader)
        send(starter, {__MODULE__, ref, :ended, own(nil, stages, opts)})
      end)

    receive do
      {__MODULE__, ^ref, :started, job} ->
        Process.demonitor(monitor, [:flush])
        {:ok, job}

      {__MODULE__, ^ref, :ended, {:error, _} = refused} ->
        Process.demonitor(monitor, [:flush])
        refused

      {:DOWN, ^monitor, :process, ^owner, _reason} ->
        {:error, :crashed}
    end
  end

  # ---- the owner ----

  # `caller` is the line a foreground job runs for, `nil` for a background job.
  defp own(caller, stages, opts) do
    watch = if caller, do: Process.monitor(caller)
    Process.flag(:trap_exit, true)
    spec = Keyword.get(opts, :budget, @default_budget)

    with :ok <- count(stages),
         {:ok, images} <- images(stages),
         {:ok, root, names} <- Pipes.open(length(stages) + 2) do
      {pipes, [err]} = Enum.split(names, length(stages) + 1)

      job = %{
        root: root,
        pipes: pipes,
        err: err,
        conns: [],
        jobs: %{},
        made: [],
        endings: %{},
        read: %{},
        killed: false
      }

      try do
        with :ok <- room(length(stages)),
             {:ok, job} <- make_pipes(job),
             {:ok, job} <- connect(job, length(stages)),
             {:ok, job} <- launch(job, stages, images, spec) do
          command = command(stages)
          id = Jobs.add(self(), caller, command)
          Process.put(:redoubt_job, id)

          with {starter, ref} <- Keyword.get(opts, :started),
               do: send(starter, {__MODULE__, ref, :started, %Redoubt.Job{id: id, command: command}})

          output = Keyword.get(opts, :output, :capture)
          readers = start_readers(job, Keyword.get(opts, :input), output)
          wait(job, readers, watch, length(stages))
        else
          {:error, name, job} ->
            remove_pipes(job)
            {:error, name}

          {:error, _name} = refused ->
            refused
        end
      after
        Enum.each(job_budgets(), &Budget.destroy/1)
        # The last job to let go of piped ends it, so its pages are the session's again.
        Pipes.release()
      end
      |> ended(caller)
    end
  end

  # The job's end, told to the session's jobs once every budget of it is destroyed and piped let
  # go: what an interrupted line waits for (`Redoubt.Jobs.settle/1`).
  defp ended(result, caller) do
    if id = Process.get(:redoubt_job), do: Jobs.ended(id, summary(result, caller))
    result
  end

  # The pipeline as it would be typed: its stages' words, with "|" between.
  defp command(stages) do
    stages |> Enum.map_join(" | ", fn {name, args} -> Enum.join([name | args], " ") end)
  end

  # What the session's jobs keep of a job's end: a background job's output and standard error,
  # for its await; a foreground job's went to its line.
  defp summary({:ok, result}, nil) do
    {stdout, out_dropped} = result.output || {"", 0}
    {stderr, err_dropped} = result.errors || {"", 0}

    %{
      stdout: stdout,
      stderr: stderr,
      dropped: {out_dropped, err_dropped},
      endings: result.endings,
      killed: result.killed
    }
  end

  defp summary({:ok, result}, _caller), do: %{endings: result.endings, killed: result.killed}
  defp summary({:error, name}, _caller), do: %{endings: [], error: name}

  defp count(stages) when stages == [], do: {:error, :empty_stage}
  defp count(stages) when length(stages) > @max_stages, do: {:error, :too_many}
  defp count(_stages), do: :ok

  # A process for each stage, left in the session's budget once piped runs: a pipeline the budget
  # cannot hold is refused before any stage starts.
  defp room(n) do
    with {:ok, own} <- Budget.own(),
         {:ok, %{processes: {limit, used}}} <- Budget.usage(own) do
      if n <= limit - used, do: :ok, else: {:error, :out_of_processes}
    else
      _no_budget -> :ok
    end
  end

  defp images(stages) do
    Enum.reduce_while(stages, {:ok, []}, fn {name, _args}, {:ok, acc} ->
      case read("/boot/" <> name) do
        {:ok, image} -> {:cont, {:ok, [image | acc]}}
        {:error, _} = error -> {:halt, error}
      end
    end)
    |> case do
      {:ok, images} -> {:ok, Enum.reverse(images)}
      error -> error
    end
  end

  defp read(path) do
    case File.read(path) do
      {:ok, bytes} -> {:ok, bytes}
      {:error, :enoent} -> {:error, :not_found}
      {:error, _} -> {:error, :refused}
    end
  end

  defp path(name, end_), do: Pipes.prefix() <> "/" <> name <> "/" <> end_

  # Every pipe's directory, made before any connection to it is minted.
  defp make_pipes(job) do
    Enum.reduce_while(job.pipes ++ [job.err], {:ok, job}, fn name, {:ok, job} ->
      case File.mkdir(Pipes.prefix() <> "/" <> name) do
        :ok -> {:cont, {:ok, %{job | made: [name | job.made]}}}
        {:error, _} -> {:halt, {:error, :refused, job}}
      end
    end)
  end

  # Each stage's three connections, each rooted at one end: {stdin, stdout, stderr}, in stage
  # order, with the ids the session lets them go by.
  defp connect(job, n) do
    Enum.reduce_while(1..n//1, {:ok, job}, fn i, {:ok, job} ->
      ends = [
        Enum.at(job.pipes, i - 1) <> "/r",
        Enum.at(job.pipes, i) <> "/w",
        job.err <> "/w"
      ]

      case mint_all(job.root, ends, []) do
        {:ok, minted} -> {:cont, {:ok, %{job | conns: job.conns ++ [minted]}}}
        {:error, minted} -> {:halt, {:error, :refused, %{job | conns: job.conns ++ [minted]}}}
      end
    end)
    |> case do
      {:error, name, job} ->
        let_go_all(job)
        {:error, name, job}

      ok ->
        ok
    end
  end

  defp mint_all(_root, [], minted), do: {:ok, Enum.reverse(minted)}

  defp mint_all(root, [end_ | rest], minted) do
    case NinepCommon.new_connection(root, end_, 0) do
      {:ok, %{conn: conn, id: id}} -> mint_all(root, rest, [{conn, id} | minted])
      _refused -> {:error, Enum.reverse(minted)}
    end
  end

  # The stages, last first, so that each reader is there before its writer: each in a budget of
  # its own, its namespace its three streams and nothing else.
  defp launch(job, stages, images, spec) do
    stages
    |> Enum.zip(images)
    |> Enum.zip(job.conns)
    |> Enum.with_index()
    |> Enum.reverse()
    |> Enum.reduce_while({:ok, job}, fn {{{{_name, args}, image}, [{i, _}, {o, _}, {e, _}]}, index},
                                        {:ok, job} ->
      with {:ok, budget} <- Budget.carve(spec),
           remember(budget),
           namespace = [{"/dev/stdin", i}, {"/dev/stdout", o}, {"/dev/stderr", e}],
           {:ok, ref} <- :redoubt.launch(%{image: image, budget: budget, namespace: namespace, args: args}) do
        {:cont, {:ok, %{job | jobs: Map.put(job.jobs, ref, %{index: index, budget: budget})}}}
      else
        {:error, name} ->
          let_go_all(job)
          {:halt, {:error, name, job}}
      end
    end)
  end

  # Every budget carved for the job, kept in the owner's dictionary so the `after` destroys them
  # however the owner leaves.
  defp remember(budget) do
    Process.put(:redoubt_pipeline_budgets, [budget | job_budgets()])
    true
  end

  defp job_budgets, do: Process.get(:redoubt_pipeline_budgets, [])

  # ---- the session's ends ----

  # A background job's standard error is kept as its output is; a foreground job's is drawn.
  defp start_readers(job, input, output) do
    source = Enum.at(job.pipes, 0)
    sink = List.last(job.pipes)

    errors =
      case output do
        {:kept, _max} -> output
        {:records, _host} -> {:kept, @kept_bytes}
        _drawn -> :console
      end

    %{
      feeder: spawn_link(fn -> feed(path(source, "w"), input) end),
      sink: spawn_link(fn -> exit({:read, drain(path(sink, "r"), output)}) end),
      err: spawn_link(fn -> exit({:read, drain(path(job.err, "r"), errors)}) end),
      # A host's reader may be waiting for its host to ask for more: at the job's end nothing it
      # holds is wanted, so it is not waited for.
      paced: match?({:records, _host}, output)
    }
  end

  # What the first stage reads. Its write end is held while this writes, and let go when it
  # closes, which is the first stage's end of its input.
  defp feed(path, input) do
    {:ok, file} = File.open(path, [:append, :binary, :raw])

    try do
      case input do
        nil -> :ok
        :console -> console(file)
        {:records, host} -> records_in(file, host)
        lines -> Enum.each(lines, &IO.binwrite(file, line(&1)))
      end
    after
      File.close(file)
    end
  end

  defp line(line) when is_binary(line), do: [line, ?\n]
  defp line(other), do: [to_string(other), ?\n]

  # The person's lines, until Ctrl+D: the shell's driver hands them over while this process holds
  # its feed (`Redoubt.Shell.Driver`), and keeps the interrupt keys for the shell.
  defp console(file) do
    case Redoubt.Shell.Driver.open_feed() do
      :ok -> console_lines(file)
      :none -> :ok
    end
  end

  defp console_lines(file) do
    receive do
      {:redoubt_feed, :data, text} ->
        IO.binwrite(file, text)
        console_lines(file)

      {:redoubt_feed, :eof} ->
        :ok
    end
  end

  # What the host hands the first stage, each write answered once it is written.
  defp records_in(file, host) do
    ref = Process.monitor(host)
    send(host, {__MODULE__, :stdin, self()})
    records_loop(file, host, ref)
  end

  defp records_loop(file, host, ref) do
    receive do
      {:write, bytes} when is_binary(bytes) ->
        IO.binwrite(file, bytes)
        send(host, {__MODULE__, :written, byte_size(bytes)})
        records_loop(file, host, ref)

      {:DOWN, ^ref, :process, _host, _reason} ->
        :ok
    end
  end

  # What a pipe holds until the end of its stream: returned; kept up to a bound, with the count of
  # the bytes past it, `{bytes, dropped}`; or written as the line's own output.
  defp drain(path, output) do
    {:ok, file} = File.open(path, [:read, :binary, :raw])

    try do
      case output do
        {:kept, max} -> keep_loop(file, max, [], 0)
        {:records, host} -> records_out(file, host, Process.monitor(host))
        output -> drain_loop(file, output, [], <<>>)
      end
    after
      File.close(file)
    end
  end

  # The stream to its end, past the bound too, so its writer is never held: what is past it is
  # counted, not kept.
  defp keep_loop(file, room, acc, dropped) do
    case IO.binread(file, @read_bytes) do
      data when is_binary(data) ->
        kept = binary_part(data, 0, min(room, byte_size(data)))
        keep_loop(file, room - byte_size(kept), [acc | kept], dropped + byte_size(data) - byte_size(kept))

      _eof_or_error ->
        {IO.iodata_to_binary(acc), dropped}
    end
  end

  # Each read handed to the host, the next only once it asks for more: until then the pipe fills,
  # and the stage's write waits.
  defp records_out(file, host, ref) do
    case IO.binread(file, @read_bytes) do
      data when is_binary(data) ->
        send(host, {__MODULE__, :stdout, self(), data})

        receive do
          {__MODULE__, :more} -> records_out(file, host, ref)
          {:DOWN, ^ref, :process, _host, _reason} -> nil
        end

      _eof_or_error ->
        nil
    end
  end

  defp drain_loop(file, output, acc, held) do
    case IO.binread(file, @read_bytes) do
      data when is_binary(data) and output == :capture ->
        drain_loop(file, output, [acc | data], held)

      data when is_binary(data) ->
        drain_loop(file, output, acc, show(held <> data))

      _eof_or_error when output == :capture ->
        IO.iodata_to_binary(acc)

      _eof_or_error ->
        if held != <<>>, do: IO.write(latin1(held))
        nil
    end
  end

  # Writes what is UTF-8 of `bytes`, a byte that is not as the Latin-1 character it is, and holds
  # a sequence the read cut, for the next.
  defp show(bytes) do
    case :unicode.characters_to_binary(bytes) do
      text when is_binary(text) ->
        IO.write(text)
        <<>>

      {:incomplete, text, rest} ->
        IO.write(text)
        rest

      {:error, text, <<byte, rest::binary>>} ->
        IO.write([text, <<byte::utf8>>])
        show(rest)
    end
  end

  defp latin1(bytes), do: for(<<byte <- bytes>>, into: <<>>, do: <<byte::utf8>>)

  # ---- waiting ----

  defp wait(job, readers, watch, n) do
    receive do
      {:exit, ref, cause, code} when is_map_key(job.jobs, ref) ->
        %{index: index, budget: budget} = job.jobs[ref]
        usage = with {:ok, u} <- Budget.usage(budget), do: u, else: (_ -> nil)
        job = ended(job, ref, index, {cause, code}, usage)

        if index == n - 1 do
          complete(job, readers, n)
        else
          wait(job, readers, watch, n)
        end

      # Killed (`Redoubt.Job.kill/1`): every stage's budget is destroyed now, as at the job's end.
      {:redoubt_job, :kill} ->
        complete(%{job | killed: true}, readers, n)

      # The line that ran the job has ended: so does the job.
      {:DOWN, ^watch, :process, _caller, _reason} ->
        Process.exit(readers.feeder, :kill)
        let_go_all(job)
        remove_pipes(job)
        {:error, :ended}

      # A reader that reached the end of its stream early (piped ended): kept for the job's end.
      {:EXIT, pid, {:read, result}} when pid in [readers.sink, readers.err] ->
        wait(%{job | read: Map.put(job.read, pid, result)}, readers, watch, n)

      {:EXIT, pid, _crashed} when pid in [readers.sink, readers.err] ->
        let_go_all(job)
        remove_pipes(job)
        {:error, :crashed}

      {:EXIT, _pid, _reason} ->
        wait(job, readers, watch, n)
    end
  end

  # A stage's notice: its connections are let go, so its reader reads the end of its stream and its
  # writer's next write is refused.
  defp ended(job, ref, index, ending, usage) do
    let_go(job, Enum.at(job.conns, index))

    %{
      job
      | jobs: Map.delete(job.jobs, ref),
        conns: List.replace_at(job.conns, index, []),
        endings: Map.put(job.endings, index, {ending, usage})
    }
  end

  # The last stage has ended: every other stage is ended too, and the job's output read to its end.
  defp complete(job, readers, n) do
    Enum.each(job.jobs, fn {_ref, %{budget: budget}} -> Budget.destroy(budget) end)
    job = collect_notices(job)
    let_go_all(job)
    if readers.paced, do: Process.exit(readers.sink, :kill)
    output = reader_result(job, readers.sink)
    errors = reader_result(job, readers.err)
    Process.exit(readers.feeder, :kill)
    remove_pipes(job)
    endings = Enum.map(0..(n - 1)//1, fn i -> Map.get(job.endings, i, {{:killed, 0}, nil}) end)

    {:ok,
     %{
       output: output,
       errors: errors,
       killed: job.killed,
       endings: Enum.map(endings, &elem(&1, 0)),
       usage: Enum.map(endings, &elem(&1, 1))
     }}
  end

  defp collect_notices(%{jobs: jobs} = job) when map_size(jobs) == 0, do: job

  defp collect_notices(job) do
    receive do
      {:exit, ref, cause, code} when is_map_key(job.jobs, ref) ->
        %{index: index} = job.jobs[ref]
        collect_notices(ended(job, ref, index, {cause, code}, nil))
    after
      @notice_wait_ms -> job
    end
  end

  defp reader_result(%{read: read}, pid) when is_map_key(read, pid), do: read[pid]

  defp reader_result(_job, pid) do
    receive do
      {:EXIT, ^pid, {:read, result}} -> result
      {:EXIT, ^pid, _crashed} -> nil
    end
  end

  defp let_go(job, conns) do
    Enum.each(conns, fn {_conn, id} -> NinepCommon.disconnect(job.root, id) end)
  end

  defp let_go_all(job), do: Enum.each(job.conns, &let_go(job, &1))

  defp remove_pipes(job), do: Enum.each(job.made, &File.rmdir(Pipes.prefix() <> "/" <> &1))
end
