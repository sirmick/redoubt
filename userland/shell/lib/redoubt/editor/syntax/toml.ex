defmodule Redoubt.Editor.Syntax.Toml do
  @moduledoc """
  TOML, for the editor's highlighting (`Redoubt.Editor.Syntax`): `.toml`. A table's line,
  `[name]`, is a heading; a `'` string takes no escapes.
  """

  @behaviour Redoubt.Editor.Syntax

  alias Redoubt.Editor.Syntax

  @table Syntax.table(
           comment: ["#"],
           strings: [
             {~S|"""|, ~S|"""|, :string, true, true},
             {"'''", "'''", :string, true, false},
             {~S|"|, ~S|"|, :string, false, true},
             {"'", "'", :string, false, false}
           ],
           constants: ~w(true false inf nan),
           heading: true
         )

  @impl Syntax
  def line(text, state), do: Syntax.scan(@table, text, state)
end
