defmodule Redoubt.Shell.Session do
  @moduledoc """
  The session's own commands, imported at the prompt: its namespace (docs/userland/sessions.md,
  "Namespaces"), who it is and its labels ("What a session is told"), printing without the pager,
  and running native programs and pipelines of them (docs/userland/shell.md, "The shell in a
  session", "Native programs and pipes").
  Each is a thin layer over `Redoubt.Namespace`, `Redoubt.Process`, `Redoubt.Pipeline` and beamlet's
  natives, and adds no authority.
  """

  use Redoubt.Commandlet, area: "Session"

  # beamlet's natives (docs/userland/beamlet.md, "Natives"): no module, so nothing to check at compile time.
  @compile {:no_warn_undefined, :redoubt}

  alias Redoubt.Util.Lines

  @summary "Show the namespace"
  @help """
  The session's namespace, one line per entry: the path, and beside it the handle's name if it was
  also handed as a named handle. Then the named handles, each by its name.
  """
  @examples [{"ns()", "what this session can reach, by path"}]
  defcommand ns() do
    table = Redoubt.Namespace.table()
    width = table |> Enum.map(fn {path, _, _} -> String.length(path) end) |> Enum.max(fn -> 0 end)

    table
    |> Enum.map(fn
      {path, name, _} when name in [nil, path] -> path
      {path, name, _} -> String.pad_trailing(path, width) <> "  " <> name
    end)
    |> Lines.new()
  end

  @summary "Find the connection a path resolves to"
  @help """
  The connection the longest matching prefix of path names, and the rest of the path below it:
  `{connection, rest}`. A name with no `/` gives the named handle of that name. Refused by name:
  `{:error, :not_found}` where nothing is bound.
  """
  @args path: "an absolute path, or a handle's name"
  @examples [{~S'{home, _rest} = ns_lookup("/home/alice")', "the home volume's connection"}]
  defcommand ns_lookup(path :: string) do
    case Redoubt.Namespace.lookup(path) do
      {:ok, handle, rest} -> {handle, rest}
      {:error, _} = error -> error
    end
  end

  @summary "Bind a connection at another path"
  @help """
  Puts a connection the session already holds at prefix as well: the same connection, under
  another name. It creates no authority, and changes nothing for any other session.
  """
  @args prefix: "a clean absolute path", connection: "a connection, as ns_lookup gives one"
  @examples [{~S'bind("/h", home)', "the home volume, at /h too"}]
  defcommand bind(prefix :: string, connection :: handle) do
    Redoubt.Namespace.bind(prefix, connection)
  end

  @summary "Who this session is"
  @help """
  The principal this session is logged in as, with its context's name after a dot when it is a
  named one (`alice.work`), as the steward told the session when it started it. nil for a VM that
  is no session, as on a host. It is what the session was told, not what it can reach: that is
  its namespace.
  """
  @examples [{"whoami()", "the principal, as \"alice\""}]
  defcommand whoami() do
    case identity() do
      %{principal: principal, context: context} when is_binary(context) -> principal <> "." <> context
      %{principal: principal} -> principal
      nil -> nil
    end
  end

  @summary "The session's labels"
  @help """
  The labels this session's budget carries, by name: none for a plain session, the vault's for a
  vault session. The set is the kernel's, fixed when the session's budget was made; the names are
  the steward's, and a label it named no name for shows as its number.
  """
  @examples [{"labels()", "[\"alice-secrets\"] in a vault session"}]
  defcommand labels() do
    names = Map.new(Map.get(identity() || %{}, :labels, []), fn {name, id} -> {id, name} end)
    Enum.map(kernel_labels(), &Map.get(names, &1, &1))
  end

  @summary "Print a value without the pager"
  @help """
  Prints value as the prompt would, but lines however many there are, a screenful or not,
  straight to the console and never into the pager. Returns :ok.
  """
  @args value: "what to print: lines, or any value"
  @examples [{~S'cat("big.log") |> out()', "the whole file, scrolling past"}]
  defcommand out(value :: term) do
    Redoubt.Shell.Printer.out(value)
    :ok
  end

  @summary "Run a native program and wait for it"
  @help """
  Runs the program /boot/name with args, in a budget carved from the session's, and waits for it
  to end. It reads the lines you type, until Ctrl+D on an empty line, and what it writes, its
  standard output and its standard error, is drawn as the line's own output: a control character
  in it shows as itself, never acts. It never holds the console. Returns how it ended,
  `{:exited, code}`, `{:faulted, cause}` or `{:killed, 0}`, and what its budget held as it ended;
  the budget is then destroyed, and everything in it ends.
  """
  @args name: "the program's name in /boot", args: "its arguments"
  @examples [{~S'exec("hello", ["world"])', "run /boot/hello world"}]
  defcommand exec(name :: string, args :: many(string) \\ []) do
    case Redoubt.Process.run(name, args) do
      {:ok, ending, usage} -> {ending, usage}
      {:error, _} = error -> error
    end
  end

  @summary "Run a native program that draws a screen"
  @help """
  Runs the program /boot/name with args as a full-screen program, in a budget carved from the
  session's. It draws by sending frames of cells on its standard output, which the session
  checks and draws; anything else it sends ends it. It reads the keys you type and the screen's
  size as events on its standard input. Ctrl+\\ ends it, and so does Ctrl+C. It never holds the
  console. Returns how it ended, `{:exited, code}` or `{:faulted, cause}`, `{:error, {:refused,
  why}}` when the session refused what it sent, or nil when you ended it; its standard error is
  drawn after the screen ends.
  """
  @args name: "the program's name in /boot", args: "its arguments"
  @examples [{~S'screen("menu")', "run /boot/menu on the whole screen"}]
  defcommand screen(name :: string, args :: many(string) \\ []) do
    Redoubt.Screen.Native.run(name, args)
  end

  @summary "Run a pipeline of native programs"
  @help """
  Runs native programs from /boot joined by pipes: words is the pipeline, its stages split at
  each "|", each a program's name and its arguments. Each stage runs in a budget of its own carved
  from the session's, with its standard input, output and error and nothing else: no file, no
  console, nothing of the session's. Stage i's output is stage i + 1's input. Given lines first,
  as from cat(path) |> pipe(...), the first stage reads them; otherwise it reads nothing.

  The value is the last stage's output, as lines; the stages' standard error is drawn as it comes.
  The pipeline is done when its last stage ends: any stage still running then is ended, and
  every budget destroyed. At most #{Redoubt.Pipeline.max_stages()} stages, and no more than the
  session's processes allow.
  """
  @args input: "the lines the first stage reads, or the pipeline's words when given alone",
        words: "the pipeline: names and arguments, with \"|\" between stages"
  @examples [
    {~S'pipe(~w(sort | uniq))', "an empty sort's output, uniq'd"},
    {~S'cat("dump.bin") |> pipe(~w(parse --json))', "an untrusted parser as one stage"}
  ]
  defcommand pipe(input :: term, words :: many(string) \\ []) do
    {input, words} = if words == [], do: {nil, input}, else: {input, words}

    with true <- (is_list(words) and Enum.all?(words, &is_binary/1)) or {:error, :not_words},
         {:ok, stages} <- Redoubt.Pipeline.stages(words),
         {:ok, %{output: output, endings: endings}} <- Redoubt.Pipeline.run(stages, input: input) do
      case Enum.reject(endings, &match?({:exited, 0}, &1)) do
        [] -> lines(output || "")
        _failed -> {lines(output || ""), endings}
      end
    end
  end

  @summary "Run a pipeline in the background"
  @help """
  Starts a pipeline as pipe does, and gives its job at once: the line goes on, and so does the
  job, which neither the interrupt nor the line's end touches. It reads only the lines it is
  given, never what is typed. What it writes, and its stages' standard error, is kept, up to
  64 KiB each, for Job.await(job). Job.kill(job) ends it: its stages' budgets are destroyed, and
  nothing else.
  """
  @args input: "the lines the first stage reads, or the pipeline's words when given alone",
        words: "the pipeline: names and arguments, with \"|\" between stages"
  @examples [
    {~S'job = bg(~w(build --all))', "start it, and go on; Job.await(job) gives what it wrote"},
    {~S'cat("urls.txt") |> bg(~w(fetch))', "a background stage reading the given lines"}
  ]
  defcommand bg(input :: term, words :: many(string) \\ []) do
    {input, words} = if words == [], do: {nil, input}, else: {input, words}

    with true <- (is_list(words) and Enum.all?(words, &is_binary/1)) or {:error, :not_words},
         {:ok, stages} <- Redoubt.Pipeline.stages(words),
         {:ok, job} <- Redoubt.Job.start(stages, input: input) do
      job
    end
  end

  @summary "List the session's jobs"
  @help """
  The session's pipelines, one line each: the job's number, whether it runs in the background,
  how it stands (running, or how its last stage ended) and its command. A background job is
  listed until Job.await takes what it wrote, or until 16 newer background jobs have ended
  unread too: past those the earliest is dropped, and a last line counts the jobs dropped.
  """
  @examples [{"jobs()", "what is running, and what has ended unread"}]
  defcommand jobs() do
    listed =
      Enum.map(Redoubt.Jobs.list(), fn job ->
        where = if job.background, do: "bg", else: "fg"
        "#{job.id}  #{where}  #{String.pad_trailing(to_string(job.state), 8)}  #{job.command}"
      end)

    dropped =
      case Redoubt.Jobs.dropped() do
        0 -> []
        n -> ["(#{n} ended background jobs dropped unread)"]
      end

    Lines.new(listed ++ dropped)
  end

  defp lines(output) do
    output
    |> String.split("\n")
    |> then(fn lines -> if List.last(lines) == "", do: Enum.drop(lines, -1), else: lines end)
    |> Lines.new()
  end

  # What the steward told the session of itself (`redoubt:identity/0`), or nil for a VM that is
  # no session, or has no beamlet natives.
  defp identity do
    case :redoubt.identity() do
      {:ok, identity} -> identity
      {:error, _} -> nil
    end
  rescue
    UndefinedFunctionError -> nil
  end

  # The session's label set, as the kernel stamped it.
  defp kernel_labels do
    :redoubt.labels()
  rescue
    UndefinedFunctionError -> []
  end
end
