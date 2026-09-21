defmodule IoTest do
  # Console output, inspect, interpolation, exceptions.
  defmodule MyError do
    defexception message: "custom"
  end

  def start do
    IO.puts("hello from elixir")
    IO.inspect({:tuple, [1, 2], %{a: 1}})
    name = "world"
    s = "hello #{name} #{1 + 2} #{inspect([a: 1])}"
    r1 = try do
      raise ArgumentError, "bad"
    rescue
      e in ArgumentError -> {:rescued, e.message}
    end
    r2 = try do
      raise MyError
    rescue
      e -> {:rescued, Exception.message(e)}
    end
    r3 = try do
      :erlang.error(:badarith)
    rescue
      e -> Exception.message(e)
    end
    r4 = catch_throw(fn -> throw(:x) end)
    {s, r1, r2, r3, r4, inspect(%{b: [1, 2], a: "str"}, custom_options: [sort_maps: true]), inspect(1.5), inspect(:atom), inspect('chars'),
     String.pad_leading("7", 3, "0"), String.reverse("abc"), String.split("a,b,,c", ","),
     Integer.parse("42abc"), Float.round(3.14159, 2), :io_lib.format("~p", [[1, 2]]) |> IO.iodata_to_binary()}
  end

  defp catch_throw(f) do
    try do
      f.()
    catch
      :throw, v -> {:caught, v}
    end
  end
end
