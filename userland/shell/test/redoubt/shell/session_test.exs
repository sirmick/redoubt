defmodule Redoubt.Shell.SessionTest do
  use ExUnit.Case, async: true

  import Redoubt.Shell.Session

  @moduletag :tmp_dir

  # On a host the VM is no session: the steward told it nothing, and its budget carries no label.
  test "a VM that is no session is nobody, with no labels" do
    assert whoami() == nil
    assert labels() == []
  end

  # A host's files are not a file server's: the copy is the caller's, through the VM.
  test "a copy the server would make is refused where no server copies", %{tmp_dir: dir} do
    File.write!(Path.join(dir, "a"), "x")
    assert Redoubt.File.copy_file(Path.join(dir, "a"), Path.join(dir, "b")) == {:error, :enotsup}
    refute File.exists?(Path.join(dir, "b"))
  end
end
