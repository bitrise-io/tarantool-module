//! Tarantool C API bindings for Rust.
//! This library contains the following Tarantool API's:
//!
//! - Box: [spaces](space), [indexes](index), [sequences](sequence)
//! - [Fibers: fiber attributes, conditional variables, latches, async runtime](fiber)
//! - [CoIO](coio)
//! - [Transactions](transaction)
//! - [Schema management](schema)
//! - [Protocol implementation](net_box) (`net.box`): CRUD, stored procedure call, triggers
//! - [Alternative async protocol implementation](network) (`network::client`): Async, coio based CRUD
//! - [Tuple utils](mod@tuple)
//! - [Decimal numbers](mod@decimal)
//! - [Logging](log) (see <https://docs.rs/log/>)
//! - [Error handling](error)
//! - [Stored procedures](macro@crate::proc)
//!
//! > **Caution!** The library is currently under development.
//! > API may be unstable until version 1.0 is released.
//!
//! ### Features
//!
//! - `net_box` - Enables protocol implementation (enabled by default)
//! - `schema` - Enables schema manipulation utils (WIP as for now)
//!
//! ### Prerequisites
//!
//! - rustc 1.61 or newer
//! - tarantool 2.2
//!
//! ### Stored procedures
//!
//! There are several ways Tarantool can call Rust code. It can use either a plugin, a Lua FFI module,
//! or a [stored procedure]. In this file we only cover the third option, namely Rust stored procedures.
//! Even though Tarantool always treats Rust routines just as "C functions", we keep on using the "stored procedure"
//! term as an agreed convention and also for historical reasons.
//!
//! This tutorial contains the following simple steps:
//! 1. `examples/easy` - prints "hello world";
//! 1. `examples/harder` - decodes a passed parameter value;
//! 1. `examples/hardest` - uses this library to do a DBMS insert;
//! 1. `examples/read` - uses this library to do a DBMS select;
//! 1. `examples/write` - uses this library to do a DBMS replace.
//!
//! Our examples are a good starting point for users who want to confidently start writing their own stored procedures.
//!
//! ### Example
//!
//! After getting the prerequisites installed, follow these steps:
//!
//! Create a Cargo project:
//! ```shell script
//! $ cargo init --lib
//! ```
//!
//! Add the following lines to `Cargo.toml`:
//! ```toml
//! [package]
//! name = "easy"
//! version = "0.1.0"
//! edition = "2018"
//! # author, license, etc
//!
//! [dependencies]
//! tarantool = "0.6.0" # (1)
//! serde = "1.0" # (2)
//!
//! [lib]
//! crate-type = ["cdylib"] # (3)
//! ```
//!
//! 1. Add the `tarantool` library to the dependencies;
//! 1. Optionally add [Serde](https://!github.com/serde-rs/serde) to the dependencies.
//! This is only required if you want to use Rust structures as tuple values (see [this example](#harder));
//! 1. Compile the dynamic library.
//!
//! Requests will be done using Tarantool as a client. Start Tarantool, and enter the following requests:
//! ```lua
//! box.cfg{listen=3306}
//! box.schema.space.create('capi_test')
//! box.space.capi_test:create_index('primary')
//! net_box = require('net.box')
//! capi_connection = net_box:new(3306)
//! ```
//!
//! Note: create a space named `capi_test` and establish the connection named `capi_connection` to the same instance.
//!
//! Leave the client running. It will be used to enter more requests later.
//!
//! Edit the `lib.rs` file and add the following lines:
//! ```rust
//! #[tarantool::proc]
//! fn easy() {
//!     println!("hello world");
//! }
//!
//! #[tarantool::proc]
//! fn easy2() {
//!     println!("hello world -- easy2");
//! }
//! ```
//!
//! Compile the program:
//! ```shell script
//! $ cargo build
//! ```
//!
//! Start another shell. Change directory (`cd`) so that it is the same as the directory that the client is running in.
//! Copy the compiled library from `target/debug` to the current directory and rename it to `easy.so`
//!
//! Now go back to the client and execute these requests:
//! ```lua
//! box.schema.func.create('easy', {language = 'C'})
//! box.schema.user.grant('guest', 'execute', 'function', 'easy')
//! capi_connection:call('easy')
//! ```
//!
//! Consult the documentation of
//! [box.schema.func.create()](https://!www.tarantool.io/en/doc/2.2/reference/reference_lua/box_schema/#box-schema-func-create),
//! [box.schema.user.grant()](https://!www.tarantool.io/en/doc/2.2/reference/reference_lua/box_schema/#box-schema-user-grant)
//! and [conn:call()](https://!www.tarantool.io/en/doc/2.2/reference/reference_lua/net_box/#net-box-call) for more details.
//!
//! The matter in hand is the `capi_connection:call('easy')` function, which has three features.
//!
//! One is to find the 'easy' function, which is easy indeed since by default Tarantool looks inside the current directory
//! for a file named `easy.so`.
//!
//! Another is to call the 'easy' function. Since the `easy()` function in `lib.rs` begins with `println!("hello world")`,
//! the words "hello world" will be printed in the terminal.
//!
//! The third feature is to make sure the call was successful. Since the `easy()` function in `lib.rs`
//! ends with return 0, there is no error message to display and therefore the request is over.
//!
//! The result should look like this:
//! ```text
//! tarantool> capi_connection:call('easy')
//! hello world
//! ---
//! - []
//! ...
//! ```
//!
//! Now let's call the other function in lib.rs - `easy2()`. This is almost the same as the `easy()`
//! function, but with a difference: when the file name is not the same as the function name,
//! we have to specify _{file-name}_._{function-name}_.
//! ```lua
//! box.schema.func.create('easy.easy2', {language = 'C'})
//! box.schema.user.grant('guest', 'execute', 'function', 'easy.easy2')
//! capi_connection:call('easy.easy2')
//! ```
//!
//! ... and this time the result will be `hello world -- easy2`.
//!
//! As you can see, calling a Rust function is as straightforward as it can be.
//!
//! [stored procedure]: macro@crate::proc
pub mod clock;
pub mod coio;
pub mod datetime;
pub mod decimal;
#[doc(hidden)]
pub mod define_str_enum;
pub mod error;
pub mod ffi;
pub mod fiber;
pub mod index;
pub mod log;
#[doc(hidden)]
pub mod msgpack;
pub mod net_box;
pub mod network;
pub mod proc;
pub mod schema;
pub mod sequence;
pub mod session;
pub mod space;
pub mod sql;
#[cfg(feature = "test")]
pub mod test;
pub mod transaction;
pub mod trigger;
pub mod tuple;
pub mod util;
pub mod uuid;
#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
#[doc(hidden)]
mod va_list;

/// `#[tarantool::proc]` is a macro attribute for creating stored procedure
/// functions.
///
/// ```rust
/// #[tarantool::proc]
/// fn add(x: i32, y: i32) -> i32 {
///     x + y
/// }
/// ```
///
/// Create a "C" stored procedure from Tarantool and call it with arguments wrapped
/// within a Lua table:
/// ```lua
/// box.schema.func.create("libname.add", { language = 'C' })
/// assert(box.func['libname.add']:call({ 1, 2 }) == 3)
/// ```
///
/// # Collecting stored procedures
///
/// All stored procs defined with `#[tarantool::proc]` attribute are
/// automatically added to a global array and can be accessed via
/// `proc::all_procs` function.
/// ```no_run
/// use tarantool::proc::all_procs;
///
/// #[tarantool::proc]
/// fn my_proc() -> i32 { 69 }
///
/// let procs = all_procs();
/// assert_eq!(procs[0].name(), "my_proc");
/// ```
///
/// This can be used to generate stored procedure defintions for tarantool's
/// `box.schema.func.create`. Although there's currently no easy way to fully
/// automate this process, because of how loading dynamic modules works in
/// tarantool. To be able to access the list of procs from a module you need to
/// call a function defined in that module.
///
/// See how you can bootstrap proc definitions in example in `examples/all_procs`.
///
/// # Accepting borrowed arguments
///
/// It can sometimes be more efficient to borrow the procedure's arguments
/// rather than copying them. This usecase is supported, however it is not
/// entirely safe. Due to how stored procedures are implemented in tarantool,
/// the arguments are allocated in a volatile region of memory, which can be
/// overwritten by some tarantool operations. Therefore you cannot rely on the
/// borrowed arguments being valid for the lifetime of the procedure call.
///
/// This proc is safe, because the data is accessed before any other calls to
/// tarantool api:
/// ```no_run
/// #[tarantool::proc]
/// fn strlen(s: &str) -> usize {
///     s.len()
/// }
/// ```
///
/// This one however is unsafe:
/// ```no_run
/// use tarantool::{error::Error, index::IteratorType::Eq, space::Space};
/// use std::collections::HashSet;
///
/// #[tarantool::proc]
/// fn count_common_friends(user1: &str, user2: String) -> Result<usize, Error> {
///     // A call to tarantool api.
///     let space = Space::find("friends_with").unwrap();
///
///     // This call is unsafe, because borrowed data `user1` is accessed
///     // after a call to tarantool api.
///     let iter = space.select(Eq, &[user1])?;
///     let user1_friends: HashSet<String> = iter
///         .map(|tuple| tuple.get(1).unwrap())
///         .collect();
///
///     // This call is safe, because `user2` is owned.
///     let iter = space.select(Eq, &[user2])?;
///     let user2_friends: HashSet<String> = iter
///         .map(|tuple| tuple.get(1).unwrap())
///         .collect();
///
///     Ok(user1_friends.intersection(&user2_friends).count())
/// }
/// ```
///
/// # Returning errors
///
/// Assuming the function's return type is [`Result`]`<T, E>` (where `E` implements
/// [`Display`]), the return values read as follows:
/// - `Ok(v)`: the stored procedure will return `v`
/// - `Err(e)`: the stored procedure will fail and `e` will be set as the last
/// Tarantool error (see also [`TarantoolError::last`])
/// ```rust
/// use tarantool::{error::Error, index::IteratorType::Eq, space::Space};
///
/// #[tarantool::proc]
/// fn get_name(id: usize) -> Result<Option<String>, Error> {
///     Ok(
///         if let Some(space) = Space::find("users") {
///             if let Some(row) = space.select(Eq, &[id])?.next() {
///                 row.get("name")
///             } else {
///                 None
///             }
///         } else {
///             None
///         }
///     )
/// }
/// ```
///
/// # Returning custom types
///
/// The return type of the stored procedure must implement the [`Return`] trait which is
/// implemented for most built-in types. To return an arbitrary type that
/// implements [`serde::Serialize`] you can use the [`ReturnMsgpack`] wrapper
/// type or the `custom_ret` attribute parameter.
/// ```no_run
/// #[derive(serde::Serialize)]
/// struct Complex {
///     re: f64,
///     im: f64,
/// }
///
/// #[tarantool::proc(custom_ret)]
/// fn sqrt(x: f64) -> Complex {
///     if x < 0. {
///         Complex { re: 0., im: x.abs().sqrt() }
///     } else {
///         Complex { re: x.sqrt(), im: 0. }
///     }
/// }
///
/// // above is equivalent to this
/// use tarantool::proc::ReturnMsgpack;
/// #[tarantool::proc]
/// fn sqrt_explicit(x: f64) -> ReturnMsgpack<Complex> {
///     ReturnMsgpack(
///         if x < 0. {
///             Complex { re: 0., im: x.abs().sqrt() }
///         } else {
///             Complex { re: x.sqrt(), im: 0. }
///         }
///     )
/// }
/// ```
///
/// # Packed arguments
///
/// By default the stored procedure unpacks the received tuple and assigns the
/// **i**th  field of the tuple to the **i**th argument. If there are fewer
/// arguments than there are fields in the input tuple, the unused tuple fields are ignored.
///
/// If you want to instead deserialize the tuple directly into your structure
/// you can use the `packed_args`
/// attribute parameter
/// ```rust
/// #[tarantool::proc(packed_args)]
/// fn sum_all(vals: Vec<i32>) -> i32 {
///     vals.into_iter().sum()
/// }
///
/// #[tarantool::proc]
/// fn sum_first_3(a: i32, b: i32, c: i32) -> i32 {
///     a + b + c
/// }
/// ```
///
/// In the above example `sum_all` will sum all the inputs values it received
/// whereas `sum_first_3` will only sum up the first 3 values
///
/// # Injecting arguments
///
/// Because the return value of the stored procedure is immediately serialized
/// it is in theory ok to return borrowed values. Rust however will not allow
/// you to return references to the values owned by the function. In that case
/// you can use an *injected* argument, which will be created just outside
/// the stored procedure and will be passed to it as a corresponding argument.
///
/// ```no_run
/// fn global_data() -> &'static [String] {
///     todo!()
/// }
///
/// #[tarantool::proc]
/// fn get_ith<'a>(
///     #[inject(global_data())]
///     data: &'a [String],
///     i: usize,
/// ) -> &'a str {
///     &data[i]
/// }
/// ```
///
/// When calling the stored procedure only the actual arguments need to be
/// specified, so in the above example `get_ith` will effectively have just 1
/// argument `i`. And `data` will be automatically injected and it's value will
/// be set to `global_data()` each time it is called.
///
/// # Debugging
///
/// There's also a `debug` attribute parameter which enables debug printing of
/// the arguments received by the stored procedure
/// ```
/// #[tarantool::proc(debug)]
/// fn print_what_you_got() {}
/// ```
///
/// The above stored procedure will just print it's any of it's arguments to
/// stderr and return immediately.
///
/// [`Result`]: std::result::Result
/// [`Display`]: std::fmt::Display
/// [`TarantoolError::last`]: crate::error::TarantoolError::last
/// [`Return`]: crate::proc::Return
/// [`ReturnMsgpack`]: crate::proc::ReturnMsgpack
pub use tarantool_proc::stored_proc as proc;
pub use tlua;

/// A re-export of [linkme] crate used inside #[`[tarantool::test]`]
/// and #[`[tarantool::proc]`] macro attributes.
pub use linkme;

/// Mark a function as a test. This will add the function to the list of tests
/// in a special global section. The tests can be accessed using
/// [`test::test_cases`] or [`test::collect_tester`].
///
/// # Example
/// ```no_run
/// #[tarantool::test]
/// fn my_test() {
///     assert!(true);
/// }
///
/// #[tarantool::test(should_panic)]
/// fn my_panicking_test() {
///     assert!(false);
/// }
/// ```
pub use tarantool_proc::test;

/// Return a global tarantool lua state.
///
/// **WARNING:** using global lua state is error prone, especially when writing
/// code that will be executed in multiple fibers. Consider using [`lua_state`]
/// instead. Use with caution if necessary.
fn global_lua() -> tlua::StaticLua {
    unsafe { tlua::Lua::from_static(ffi::tarantool::luaT_state()) }
}

/// Create a new lua state with an isolated stack. The new state has access to
/// all the global and tarantool data (Lua variables, tables, modules, etc.).
pub fn lua_state() -> tlua::LuaThread {
    global_lua().new_thread()
}

pub use error::Result;
pub type StdResult<T, E> = std::result::Result<T, E>;
