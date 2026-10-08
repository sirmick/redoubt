defmodule Redoubt.Editor.Syntax.Rust do
  @moduledoc """
  Rust, for the editor's highlighting (`Redoubt.Editor.Syntax`): `.rs`. Block comments nest. A
  `'` is plain, since it starts a lifetime as often as a character.
  """

  @behaviour Redoubt.Editor.Syntax

  alias Redoubt.Editor.Syntax

  @table Syntax.table(
           comment: ["//"],
           block: {"/*", "*/", true},
           strings: [{~S|"|, ~S|"|, :string, true, true}],
           keywords: ~w(as async await break const continue crate dyn else enum extern fn for if
             impl in let loop match mod move mut pub ref return self Self static struct super
             trait type unsafe use where while),
           constants: ~w(true false),
           capitals: :constant
         )

  @impl Syntax
  def line(text, state), do: Syntax.scan(@table, text, state)
end
