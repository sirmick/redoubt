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
    no stage outlives the line that ran it.
  - **What reaches the console** passes the shell's guard (docs/userland/shell.md, "Hostile text
    never drives the terminal"): a stage's output and standard error are written as the line's
    own output, so through the shell's driver and `Redoubt.Term`.
  """

  # beamlet's natives (docs/userland/beamlet.md, "Natives"): no module, so nothing to check at compile time.
  @compile {:no_warn_undefined, :redoubt}

  alias Redoubt.{Budget, Pipes}
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
    once; `:console` for the lines the person types, until Ctrl+D; or an enumerable of lines or
    binaries, each line written with a newline after it.
  - `:output`: `:capture` (the default) to return what the last stage writes, or `:console` to
    draw it as it comes.
  - `:budget`: each stage's budget spec, `#{inspect(@default_budget)}` when left out.

  Returns `{:ok, %{output: bytes | nil, endings: [ending], usage: [usage]}}`, a stage's usage being
  what its budget held as it ended, or `{:error, name}` when it could not start.
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

  # ---- the owner ----

  defp own(caller, stages, opts) do
    watch = Process.monitor(caller)
    Process.flag(:trap_exit, true)
    spec = Keyword.get(opts, :budget, @default_budget)

    with :ok <- count(stages),
         {:ok, images} <- images(stages),
         {:ok, root, names} <- Pipes.open(length(stages) + 2) do
      {pipes, [err]} = Enum.split(names, length(stages) + 1)
      job = %{root: root, pipes: pipes, err: err, conns: [], jobs: %{}, made: [], endings: %{}, read: %{}}

      try do
        with :ok <- room(length(stages)),
             {:ok, job} <- make_pipes(job),
             {:ok, job} <- connect(job, length(stages)),
             {:ok, job} <- launch(job, stages, images, spec) do
          readers = start_readers(job, Keyword.get(opts, :input), Keyword.get(opts, :output, :capture))
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
    end
  end

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

  defp start_readers(job, input, output) do
    source = Enum.at(job.pipes, 0)
    sink = List.last(job.pipes)

    %{
      feeder: spawn_link(fn -> feed(path(source, "w"), input) end),
      sink: spawn_link(fn -> exit({:read, drain(path(sink, "r"), output)}) end),
      err: spawn_link(fn -> exit({:read, drain(path(job.err, "r"), :console)}) end)
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

  # What a pipe holds until the end of its stream: returned, or written as the line's own output.
  defp drain(path, output) do
    {:ok, file} = File.open(path, [:read, :binary, :raw])

    try do
      drain_loop(file, output, [], <<>>)
    after
      File.close(file)
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
    output = reader_result(job, readers.sink)
    _ = reader_result(job, readers.err)
    Process.exit(readers.feeder, :kill)
    remove_pipes(job)
    endings = Enum.map(0..(n - 1)//1, fn i -> Map.get(job.endings, i, {{:killed, 0}, nil}) end)
    {:ok, %{output: output, endings: Enum.map(endings, &elem(&1, 0)), usage: Enum.map(endings, &elem(&1, 1))}}
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
