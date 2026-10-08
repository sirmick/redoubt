defmodule Redoubt.Shell.Session do
  @moduledoc """
  The session's own commands, imported at the prompt: its namespace (docs/userland/sessions.md,
  "Namespaces"), printing without the pager, and running a native program (docs/userland/shell.md,
  "The shell in a session").
  Each is a thin layer over `Redoubt.Namespace` and `Redoubt.Process`, and adds no authority.
  """

  use Redoubt.Commandlet, area: "Session"

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
end
