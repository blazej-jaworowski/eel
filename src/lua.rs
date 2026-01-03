pub use super::nvim_oxi::mlua;

use mlua::{FromLua, FromLuaMulti, Function, IntoLuaMulti, Table, Value};

pub type Result<T> = std::result::Result<T, mlua::Error>;

#[macro_export]
macro_rules! lua_tuple {
    () => {
        ()
    };

    ($( $value:tt ),* $(,)? ) => {{
        use $crate::lua_value;
        ( $( lua_value!($value) ),* )
    }};
}

#[macro_export]
macro_rules! lua_table {
    () => {{
        use $crate::nvim_oxi::mlua::{lua, Value};
        Value::Table(lua().create_table()?)
    }};

    ($( $key:expr => $value:tt ),* $(,)? ) => {{
        use $crate::{nvim_oxi::mlua::{lua, Value}, lua_value};
        Value::Table(lua().create_table_from(
            [ $( (lua_value!($key), lua_value!($value)) ),* ]
        )?)
    }};
}

#[macro_export]
macro_rules! lua_array {
    () => {{
        use $crate::nvim_oxi::mlua::{lua, Value};
        Value::Table(lua().create_table()?)
    }};

    ($( $value:tt ),* $(,)? ) => {{
        use $crate::{nvim_oxi::mlua::{lua, Value}, lua_value};
        Value::Table(lua().create_sequence_from(
            [ $( lua_value!($value) ),* ]
        )?)
    }};
}

#[macro_export]
macro_rules! lua_value {
    () => {{
        use $crate::nvim_oxi::mlua::Value;
        Value::NULL
    }};

    ({ $( $key:expr => $value:tt ),* $(,)? }) => {{
        use $crate::lua_table;
        lua_table!{$( $key => $value ),*}
    }};

    ([ $( $value:tt ),* $(,)? ]) => {{
        use $crate::lua_array;
        lua_array![$( $value ),*]
    }};

    (( $( $value:tt ),* $(,)? )) => {{
        use $crate::lua_tuple;
        lua_tuple!($( $value ),*)
    }};

    ($value:tt) => {{
        use $crate::nvim_oxi::mlua::{IntoLua, lua};
        $value.into_lua(&lua())?
    }};
}

pub fn lua_get_value_path<T: FromLua>(mut obj: Value, path: &str) -> Result<T> {
    let lua = mlua::lua();

    for part in path.split(".") {
        let table: &Table = obj
            .as_table()
            .ok_or(mlua::Error::RuntimeError("Invalid lua value path".into()))?;

        obj = table.get(part)?;
    }

    T::from_lua(obj, &lua)
}

pub fn lua_get_global_path<T: FromLua>(path: &str) -> Result<T> {
    let globals = Value::Table(mlua::lua().globals());
    lua_get_value_path(globals, path)
}

pub fn require_plugin(plugin_name: &str) -> Result<Table> {
    let require_func: Function = lua_get_global_path("require")?;
    require_func.call(plugin_name)
}

pub fn require_call_setup_val<A, R>(plugin_name: &str, args: A) -> Result<R>
where
    A: IntoLuaMulti,
    R: FromLuaMulti,
{
    require_plugin(plugin_name)?
        .get::<Function>("setup")?
        .call(args)
}

pub fn require_call_setup<A>(plugin_name: &str, args: A) -> Result<()>
where
    A: IntoLuaMulti,
{
    require_call_setup_val(plugin_name, args)
}
