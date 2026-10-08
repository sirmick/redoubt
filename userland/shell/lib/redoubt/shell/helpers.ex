defmodule Redoubt.Shell.Helpers do
  @moduledoc """
  The file commands, imported at the prompt.

  A relative name resolves against the VM's working directory, which `cd/1` sets and `pwd/0`
  reports; the VM is the session, so it is the session's. A command that changes files returns
  `:ok`; one that asks returns what it found: lines of names, or a `Redoubt.Shell.Stat`. None asks
  whether you are sure: a command that removes a whole tree says so in its name, `rm_rf`.
  """

  use Redoubt.Commandlet, area: "Files"

  alias Redoubt.Commandlet.UsageError
  alias Redoubt.Shell.Stat
  alias Redoubt.Util.Lines

  @summary "Show the working directory"
  @help """
  The directory relative names resolve against.
  """
  @examples [{"pwd()", "where this session is"}]
  defcommand pwd() do
    File.cwd!()
  end

  @summary "Change the working directory"
  @help """
  Moves the working directory, and returns the new one. A relative dir is taken from the
  current one.
  """
  @args dir: "where to go"
  @examples [{~S'cd("logs")', "into logs, below here"}, {~S'cd("..")', "up one"}]
  defcommand cd(dir :: path) do
    File.cd!(dir)
    File.cwd!()
  end

  @summary "List a directory"
  @help """
  The names in a directory, in order, as lines.
  """
  @args dir: "the directory; here when left out"
  @examples [{"ls()", "the names here"}, {~S'ls("logs") |> grep(".log")', "the .log names in logs"}]
  defcommand ls(dir :: path \\ ".") do
    dir |> File.ls!() |> Enum.sort() |> Lines.new()
  end

  @summary "List a directory and everything below it"
  @help """
  Every path below dir, a directory before what it holds, and in order at each level. The paths
  start with dir, or, for the working directory, are relative to it. A symbolic link is listed,
  never followed.
  """
  @args dir: "where to start; here when left out"
  @examples [
    {"ls_r()", "everything below here"},
    {~S'ls_r("logs") |> Enum.filter(&(stat(&1).size > 1_000_000))', "the files in logs over a megabyte"}
  ]
  defcommand ls_r(dir :: path \\ ".") do
    dir |> walk(if(dir == ".", do: "", else: dir)) |> Lines.new()
  end

  @summary "Find paths by name"
  @help """
  The paths below dir whose last part holds the pattern: a string anywhere in it, or a regex
  matching it. Everything below dir is looked at, as ls_r lists it.
  """
  @args dir: "where to look", pattern: "a string to find in a name, or a regex to match one"
  @examples [
    {~S'find(".", ".log")', "every path below here whose name holds .log"},
    {~S'find("src", ~r/_test\.exs$/)', "the test files below src"}
  ]
  defcommand find(dir :: path, pattern :: pattern) do
    dir |> ls_r() |> Stream.filter(&name_matches?(Path.basename(&1), pattern)) |> Lines.new()
  end

  @summary "Expand a wildcard to the paths it matches"
  @help """
  The paths matching the pattern, in order: * matches within a name, ** across directories,
  ? one character, and {a,b} either. A name starting with a dot is matched only by a pattern
  that starts it with a dot too.
  """
  @args pattern: "the wildcard, as logs/*.log"
  @examples [
    {~S'glob("logs/*.log")', "the .log files in logs"},
    {~S'glob("**/*.txt")', "every .txt below here"}
  ]
  defcommand glob(pattern :: string) do
    pattern |> Path.wildcard() |> Enum.sort() |> Lines.new()
  end

  @summary "Show a file's type, size and time"
  @help """
  A Redoubt.Shell.Stat: the path as given, its type (regular, directory, symlink or other), its
  size in bytes, and when it last changed. There is no mode and no owner.
  """
  @args path: "the file"
  @examples [{~S'stat("notes.txt").size', "how many bytes notes.txt holds"}]
  defcommand stat(path :: path) do
    info = File.stat!(path, time: :posix)
    %Stat{path: path, type: info.type, size: info.size, mtime: DateTime.from_unix!(info.mtime)}
  end

  @summary "Copy a file"
  @help """
  Copies src to dst. When dst is an existing directory, the copy goes into it and keeps its name.
  Within one volume the file server makes the copy itself; across volumes, or over a file that is
  there, the bytes pass through the session.
  """
  @args src: "the file to copy", dst: "where the copy goes: a file, or a directory to put it in"
  @examples [
    {~S'cp("a.txt", "b.txt")', "copy a.txt to b.txt"},
    {~S'cp("a.txt", "backup")', "copy a.txt into backup, as backup/a.txt"}
  ]
  defcommand cp(src :: path, dst :: path) do
    dst = target(src, dst)

    case Redoubt.File.copy_file(src, dst) do
      {:ok, _bytes} -> :ok
      {:error, _cannot} -> File.cp!(src, dst)
    end
  end

  @summary "Move or rename a file"
  @help """
  Moves src to dst. When dst is an existing directory, what is moved goes into it and keeps its
  name. Within one volume it is a rename, done at once. Across volumes it is a copy and then a
  removal: not atomic, so a failure between them leaves both.
  """
  @args src: "what to move: a file, or a directory", dst: "its new name, or a directory to move it into"
  @examples [
    {~S'mv("old.txt", "new.txt")', "rename old.txt to new.txt"},
    {~S'mv("a.txt", "archive")', "move a.txt into archive"}
  ]
  defcommand mv(src :: path, dst :: path) do
    dst = target(src, dst)

    case File.rename(src, dst) do
      :ok ->
        :ok

      {:error, :exdev} ->
        copy_and_remove(src, dst)

      {:error, reason} ->
        raise File.RenameError, reason: reason, action: "rename", source: src, destination: dst
    end
  end

  @summary "Remove a file, or an empty directory"
  @help """
  Removes path: a file, or a directory with nothing in it. A directory that holds anything is
  left as it is, and the call fails; rm_rf removes a whole tree. A file that is open can be
  removed.
  """
  @args path: "what to remove"
  @examples [{~S'rm("old.log")', "remove old.log"}, {~S'rm("empty")', "remove the empty directory empty"}]
  defcommand rm(path :: path) do
    case File.lstat(path) do
      {:ok, %File.Stat{type: :directory}} ->
        if File.ls!(path) != [],
          do: raise(File.Error, reason: :enotempty, action: "remove directory", path: path)

        File.rmdir!(path)

      _file_or_nothing ->
        File.rm!(path)
    end
  end

  @summary "Remove a directory and everything in it"
  @help """
  Removes path and everything below it, and succeeds when there was nothing there. It refuses
  only /, which would be everything the session may change.
  """
  @args path: "the tree to remove"
  @examples [{~S'rm_rf("build")', "remove build and all it holds"}]
  defcommand rm_rf(path :: path) do
    if root?(path), do: raise(UsageError, message: "rm_rf: refuses to remove /\nusage: rm_rf(path)")
    File.rm_rf!(path)
    :ok
  end

  @summary "Make a directory"
  @help """
  Makes the directory path, whose parent must already be there, and fails if path is. mkdir_p
  makes the parents too.
  """
  @args path: "the directory to make"
  @examples [{~S'mkdir("logs")', "make logs, here"}]
  defcommand mkdir(path :: path) do
    File.mkdir!(path)
  end

  @summary "Make a directory, and its parents"
  @help """
  Makes the directory path, and every directory above it that is not there yet. It succeeds when
  path is already a directory.
  """
  @args path: "the directory to make"
  @examples [{~S'mkdir_p("logs/2026/09")', "make logs, logs/2026 and logs/2026/09 as needed"}]
  defcommand mkdir_p(path :: path) do
    File.mkdir_p!(path)
  end

  @summary "Make an empty file, or mark a file changed"
  @help """
  Makes path an empty file if it is not there, and otherwise sets the time it last changed to
  now, leaving what it holds alone.
  """
  @args path: "the file"
  @examples [{~S'touch("notes.txt")', "make notes.txt, or mark it changed"}]
  defcommand touch(path :: path) do
    File.touch!(path)
  end

  @doc false
  # Whether rm_rf must refuse the path: / itself, however it is written.
  def root?(path), do: Path.expand(path) == "/"

  defp target(src, dst), do: if(File.dir?(dst), do: Path.join(dst, Path.basename(src)), else: dst)

  # A move between volumes: a copy, then the removal of what was copied and nothing else, so
  # anything added to src meanwhile stays there. A directory never lands on an existing one,
  # as a rename would refuse.
  defp copy_and_remove(src, dst) do
    if File.dir?(src) do
      if File.exists?(dst),
        do: raise(File.RenameError, reason: :eexist, action: "move", source: src, destination: dst)

      dst
      |> then(fn dst -> File.cp_r!(src, dst) end)
      |> Enum.map(&Path.join(src, Path.relative_to(&1, dst)))
      |> Enum.sort_by(&(-length(Path.split(&1))))
      |> Enum.each(fn path -> if File.dir?(path), do: File.rmdir(path), else: File.rm!(path) end)

      # Last, src itself, which is gone only if nothing was added to it meanwhile.
      File.rmdir(src)
    else
      File.cp!(src, dst)
      File.rm!(src)
    end

    :ok
  end

  # The paths below dir, each under prefix, a directory before what it holds.
  defp walk(dir, prefix) do
    dir
    |> File.ls!()
    |> Enum.sort()
    |> Enum.flat_map(fn name ->
      path = if prefix == "", do: name, else: Path.join(prefix, name)
      full = Path.join(dir, name)

      case File.lstat(full) do
        {:ok, %File.Stat{type: :directory}} -> [path | walk(full, path)]
        _other -> [path]
      end
    end)
  end

  defp name_matches?(name, %Regex{} = regex), do: Regex.match?(regex, name)
  defp name_matches?(name, text), do: String.contains?(name, text)
end
