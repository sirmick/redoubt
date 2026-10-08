defmodule Redoubt.Shell.Session do
  @moduledoc """
  The session's own commands, imported at the prompt: its namespace (docs/userland/sessions.md,
  "Namespaces"), who it is and its labels ("What a session is told"), printing without the pager,
  and running a native program (docs/userland/shell.md, "The shell in a session").
  Each is a thin layer over `Redoubt.Namespace`, `Redoubt.Process` and beamlet's natives, and adds
  no authority.
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
  Runs the program /boot/name with args, in a budget carved from the session's, with the
  session's console as its own, and waits for it to end. Returns how it ended, `{:exited, code}`,
  `{:faulted, cause}` or `{:killed, 0}`, and what its budget held as it ended; the budget is then
  destroyed, and everything in it ends.
  """
  @args name: "the program's name in /boot", args: "its arguments"
  @examples [{~S'exec("hello", ["world"])', "run /boot/hello world"}]
  defcommand exec(name :: string, args :: many(string) \\ []) do
    case Redoubt.Process.run(name, args) do
      {:ok, ending, usage} -> {ending, usage}
      {:error, _} = error -> error
    end
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
