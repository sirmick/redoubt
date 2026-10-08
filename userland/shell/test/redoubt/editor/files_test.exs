defmodule Redoubt.Editor.FilesTest do
  # The editor's and the file manager's file actions. Each attack's verdict is read from the file
  # system afterwards: what is there, and what it holds.
  use ExUnit.Case, async: true

  alias Redoubt.Editor.Files

  @moduletag :tmp_dir

  # Every path below `dir`, with what each file holds: the whole tree, to compare before and after.
  defp tree(dir) do
    dir
    |> Path.join("**")
    |> Path.wildcard(match_dot: true)
    |> Enum.sort()
    |> Enum.map(fn path ->
      {Path.relative_to(path, dir), if(File.dir?(path), do: :dir, else: File.read!(path))}
    end)
  end

  # A listed directory, `pane`, beside one the panes do not list, `outside`, and a second pane.
  defp seed(dir) do
    for d <- ["pane/sub", "outside", "other"], do: File.mkdir_p!(Path.join(dir, d))
    File.write!(Path.join(dir, "pane/a.txt"), "a\n")
    File.write!(Path.join(dir, "pane/sub/b.txt"), "b\n")
    File.write!(Path.join(dir, "outside/keep.txt"), "keep\n")
    {Path.join(dir, "pane"), Path.join(dir, "other")}
  end

  @crafted ["../outside", "../outside/keep.txt", "sub/b.txt", "/", "..", ".", "", "a.txt\0../outside"]

  test "a name holding / or NUL, or empty, . or .., is refused, and nothing anywhere changes", %{tmp_dir: dir} do
    {pane, other} = seed(dir)
    before = tree(dir)

    for name <- @crafted do
      refute Files.name_ok?(name), inspect(name)
      assert Files.remove(pane, name) == {:error, :bad_name}
      assert Files.mkdir(pane, name) == {:error, :bad_name}
      assert Files.copy(pane, name, other) == {:error, :bad_name}
      assert Files.move(pane, name, other) == {:error, :bad_name}
    end

    assert tree(dir) == before
  end

  test "a name with control or bidirectional characters is listed escaped, and acted on as itself",
       %{tmp_dir: dir} do
    {pane, other} = seed(dir)
    rlo = <<0x202E::utf8>>
    names = ["\e[31mred\e[0m", "evil#{rlo}txt.exe", "bell\a"]
    for name <- names, do: File.write!(Path.join(pane, name), "x")

    {:ok, entries} = Files.list(pane)
    shown = Map.new(entries, &{&1.name, &1})

    for name <- names do
      entry = shown[name]
      assert entry.ok and entry.type == :regular
      assert entry.shown == Redoubt.Term.Text.visible(name)
      refute entry.shown =~ ~r/[\x00-\x1f\x7f\x{202a}-\x{202e}\x{2066}-\x{2069}]/u
    end

    assert shown["sub"].type == :directory and shown["a.txt"].type == :regular

    assert Files.copy(pane, "\e[31mred\e[0m", other) == :ok
    assert File.read!(Path.join(other, "\e[31mred\e[0m")) == "x"
    assert Files.remove(pane, "evil#{rlo}txt.exe") == :ok
    refute File.exists?(Path.join(pane, "evil#{rlo}txt.exe"))
  end

  test "the pane operations act inside the listed directories, and overwrite nothing", %{tmp_dir: dir} do
    {pane, other} = seed(dir)

    assert Files.copy(pane, "sub", other) == :ok
    assert File.read!(Path.join(other, "sub/b.txt")) == "b\n"
    assert Files.copy(pane, "a.txt", other) == :ok
    assert Files.copy(pane, "a.txt", other) == {:error, :eexist}
    assert Files.move(pane, "a.txt", other) == {:error, :eexist}
    assert File.exists?(Path.join(pane, "a.txt"))

    assert Files.remove(other, "a.txt") == :ok
    assert Files.move(pane, "a.txt", other) == :ok
    refute File.exists?(Path.join(pane, "a.txt"))
    assert File.read!(Path.join(other, "a.txt")) == "a\n"

    assert Files.copy(dir, "pane", Path.join(dir, "pane/sub")) == {:error, :einval}
    assert Files.mkdir(pane, "new") == :ok and File.dir?(Path.join(pane, "new"))
    assert Files.remove(pane, "sub") == :ok and not File.exists?(Path.join(pane, "sub"))
    assert File.read!(Path.join(dir, "outside/keep.txt")) == "keep\n"
  end

  test "a save reaches only its own path, and leaves nothing else behind", %{tmp_dir: dir} do
    {pane, _other} = seed(dir)
    path = Path.join(pane, "a.txt")
    {:ok, %{bytes: "a\n", utf8: true, digest: digest}} = Files.read(path)
    before = tree(dir)

    assert {:ok, saved} = Files.save(path, ["edited", ?\n], digest)
    assert File.read!(path) == "edited\n"
    assert tree(dir) == List.keyreplace(before, "pane/a.txt", 0, {"pane/a.txt", "edited\n"})

    # Changed on disk since it was read: not overwritten, unless the caller says so.
    File.write!(path, "theirs\n")
    assert Files.save(path, "mine\n", saved) == {:error, :changed}
    assert File.read!(path) == "theirs\n"
    assert {:ok, _} = Files.save(path, "mine\n", :any)
    assert File.read!(path) == "mine\n"

    # A new file: saved only if nothing took its name meanwhile.
    new = Path.join(pane, "new.txt")
    assert Files.read(new) == {:error, :enoent}
    assert {:ok, _} = Files.save(new, "hello\n", :absent)
    assert Files.save(new, "again\n", :absent) == {:error, :changed}
    assert File.read!(new) == "hello\n"

    # A save that cannot be made leaves no file of its own.
    assert {:error, _} = Files.save(Path.join(pane, "missing/x.txt"), "x", :any)
    assert Enum.sort(File.ls!(pane)) == ["a.txt", "new.txt", "sub"]

    # A new file made and written, then refused its rename (a directory holding something is in
    # the way): removed, and the directory untouched.
    assert {:error, _} = Files.save(Path.join(pane, "sub"), "x", :any)
    assert Enum.sort(File.ls!(pane)) == ["a.txt", "new.txt", "sub"]
    assert File.read!(Path.join(pane, "sub/b.txt")) == "b\n"
  end

  test "a directory that refuses the write: the save fails, and raises nothing", %{tmp_dir: dir} do
    {pane, _other} = seed(dir)
    path = Path.join(pane, "a.txt")
    {:ok, %{digest: digest}} = Files.read(path)
    File.chmod!(pane, 0o555)
    before = tree(dir)

    try do
      assert {:error, _} = Files.save(path, "edited\n", digest)
      assert {:error, _} = Files.save(Path.join(pane, "new.txt"), "new\n", :absent)
      assert tree(dir) == before
    after
      File.chmod!(pane, 0o755)
    end
  end

  test "a file grown past the limit since it was read counts as changed, and is left as it is",
       %{tmp_dir: dir} do
    path = Path.join(dir, "grows")
    File.write!(path, "small\n")
    {:ok, %{digest: digest}} = Files.read(path)
    File.write!(path, :binary.copy("x", Files.max_bytes() + 1))

    assert Files.save(path, "mine\n", digest) == {:error, :changed}
    assert File.stat!(path).size == Files.max_bytes() + 1
  end

  test "what a file holds is only data: escapes and modelines come back as the bytes they are",
       %{tmp_dir: dir} do
    path = Path.join(dir, "hostile.txt")
    bytes = "# vim: set shell=/bin/sh :\n\e]0;title\a\e[2J\n# -*- eval: (rm) -*-\n\xff\xfe\n"
    File.write!(path, bytes)
    before = tree(dir)

    assert {:ok, %{bytes: ^bytes, utf8: false}} = Files.read(path)
    assert tree(dir) == before
  end

  test "a file over the limit, or one that is not a file, is refused by name", %{tmp_dir: dir} do
    big = Path.join(dir, "big")
    File.write!(big, :binary.copy("x", Files.max_bytes() + 1))
    assert Files.read(big) == {:error, {:too_large, Files.max_bytes() + 1, Files.max_bytes()}}
    assert Files.read(dir) == {:error, {:not_a_file, :directory}}
  end
end
