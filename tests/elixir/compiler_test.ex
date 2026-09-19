# Elixir's own compiler, running on the VM: compile a module from source, call it, evaluate.
defmodule CompilerTest do
  def start do
    {:ok, _} = Application.ensure_all_started(:elixir)

    [{mod, _}] =
      Code.compile_string(~S"""
      defmodule CompiledAtRuntime do
        defstruct [:name, count: 0]
        def double(xs), do: Enum.map(xs, &(&1 * 2))
        def bump(%__MODULE__{count: c} = s), do: %{s | count: c + 1}
        def describe(x) when is_integer(x), do: "int #{x}"
        def describe(x), do: "other #{inspect(x)}"
      end
      """)

    {value, _binding} = Code.eval_string("a + b * c", a: 1, b: 2, c: 3)
    quoted = Code.string_to_quoted!("fn x -> x + 1 end")

    {mod, mod.double([1, 2, 3]), mod.bump(struct(mod, name: "n")).count,
     mod.describe(7), mod.describe(:x), value, Macro.to_string(quoted)}
  end
end
