use std::fmt;
use std::borrow::Cow;

use crate::http::{uri, Method, MediaType};
use crate::route::{Handler, RouteUri, BoxFuture};
use crate::sentinel::Sentry;

/// A request handling route.
///
/// A route consists of exactly the information in its fields. While a `Route`
/// can be instantiated directly, doing so should be a rare or nonexistent
/// event. Instead, a Rocket application should use Rocket's
/// [`#[route]`](macro@crate::route) series of attributes to generate a `Route`.
///
/// ```rust
/// # #[macro_use] extern crate rocket;
/// # use std::path::PathBuf;
/// #[get("/route/<path..>?query", rank = 2, format = "json")]
/// fn route_name(path: PathBuf) { /* handler procedure */ }
///
/// use rocket::http::{Method, MediaType};
///
/// let route = routes![route_name].remove(0);
/// assert_eq!(route.name.unwrap(), "route_name");
/// assert_eq!(route.method, Some(Method::Get));
/// assert_eq!(route.uri, "/route/<path..>?query");
/// assert_eq!(route.rank, 2);
/// assert_eq!(route.format.unwrap(), MediaType::JSON);
/// ```
///
/// Note that the `rank` and `format` attribute parameters are optional. See
/// [`#[route]`](macro@crate::route) for details on macro usage. Note also that
/// a route's mounted _base_ becomes part of its URI; see [`RouteUri`] for
/// details.
///
/// # Routing
///
/// A request is _routed_ to a route if it has the highest precedence (lowest
/// rank) among all routes that [match](Route::matches()) the request. See
/// [`Route::matches()`] for details on what it means for a request to match.
///
/// Note that a single request _may_ be routed to multiple routes if a route
/// forwards. If a route fails, the request is instead routed to the highest
/// precedence [`Catcher`](crate::Catcher).
///
/// ## Collisions
///
/// Two routes are said to [collide](Route::collides_with()) if there exists a
/// request that matches both routes. Colliding routes present a routing
/// ambiguity and are thus disallowed by Rocket. Because routes can be
/// constructed dynamically, collision checking is done at
/// [`ignite`](crate::Rocket::ignite()) time, after it becomes statically
/// impossible to add any more routes to an instance of `Rocket`.
///
/// Note that because query parsing is always lenient -- extra and missing query
/// parameters are allowed -- queries do not directly impact whether two routes
/// collide.
///
/// ## Resolving Collisions
///
/// Collisions are resolved through _ranking_. Routes with lower ranks have
/// higher precedence during routing than routes with higher ranks. Thus, routes
/// are attempted in ascending rank order. If a higher precedence route returns
/// an `Outcome` of `Forward`, the next highest precedence route is attempted,
/// and so on, until a route returns `Success` or `Error`, or there are no
/// more routes to try. When all routes have been attempted, Rocket issues a
/// `404` error, handled by the appropriate [`Catcher`](crate::Catcher).
///
/// ## Default Ranking
///
/// Most collisions are automatically resolved by Rocket's _default rank_. The
/// default rank prefers static components over dynamic components in both paths
/// and queries: the _more_ static a route's path and query are, the lower its
/// rank and thus the higher its precedence.
///
/// There are three "colors" to paths and queries:
///   1. `static` - all components are static
///   2. `partial` - at least one, but not all, components are dynamic
///   3. `wild` - all components are dynamic
///
/// Static paths carry more weight than static queries. The same is true for
/// partial and wild paths. This results in the following default ranking
/// table:
///
/// | path    | query   | rank |
/// |---------|---------|------|
/// | static  | static  | -12  |
/// | static  | partial | -11  |
/// | static  | wild    | -10  |
/// | static  | none    | -9   |
/// | partial | static  | -8   |
/// | partial | partial | -7   |
/// | partial | wild    | -6   |
/// | partial | none    | -5   |
/// | wild    | static  | -4   |
/// | wild    | partial | -3   |
/// | wild    | wild    | -2   |
/// | wild    | none    | -1   |
///
/// Recall that _lower_ ranks have _higher_ precedence.
///
/// ### Example
///
/// ```rust
/// use rocket::Route;
/// use rocket::http::Method;
///
/// macro_rules! assert_rank {
///     ($($uri:expr => $rank:expr,)*) => {$(
///         let route = Route::new(Method::Get, $uri, rocket::route::dummy_handler);
///         assert_eq!(route.rank, $rank);
///     )*}
/// }
///
/// assert_rank! {
///     "/?foo" => -12,                 // static path, static query
///     "/foo/bar?a=b&bob" => -12,      // static path, static query
///     "/?a=b&bob" => -12,             // static path, static query
///
///     "/?a&<zoo..>" => -11,           // static path, partial query
///     "/foo?a&<zoo..>" => -11,        // static path, partial query
///     "/?a&<zoo>" => -11,             // static path, partial query
///
///     "/?<zoo..>" => -10,             // static path, wild query
///     "/foo?<zoo..>" => -10,          // static path, wild query
///     "/foo?<a>&<b>" => -10,          // static path, wild query
///
///     "/" => -9,                      // static path, no query
///     "/foo/bar" => -9,               // static path, no query
///
///     "/a/<b>?foo" => -8,             // partial path, static query
///     "/a/<b..>?foo" => -8,           // partial path, static query
///     "/<a>/b?foo" => -8,             // partial path, static query
///
///     "/a/<b>?<b>&c" => -7,           // partial path, partial query
///     "/a/<b..>?a&<c..>" => -7,       // partial path, partial query
///
///     "/a/<b>?<c..>" => -6,           // partial path, wild query
///     "/a/<b..>?<c>&<d>" => -6,       // partial path, wild query
///     "/a/<b..>?<c>" => -6,           // partial path, wild query
///
///     "/a/<b>" => -5,                 // partial path, no query
///     "/<a>/b" => -5,                 // partial path, no query
///     "/a/<b..>" => -5,               // partial path, no query
///
///     "/<b>/<c>?foo&bar" => -4,       // wild path, static query
///     "/<a>/<b..>?foo" => -4,         // wild path, static query
///     "/<b..>?cat" => -4,             // wild path, static query
///
///     "/<b>/<c>?<foo>&bar" => -3,     // wild path, partial query
///     "/<a>/<b..>?a&<b..>" => -3,     // wild path, partial query
///     "/<b..>?cat&<dog>" => -3,       // wild path, partial query
///
///     "/<b>/<c>?<foo>" => -2,         // wild path, wild query
///     "/<a>/<b..>?<b..>" => -2,       // wild path, wild query
///     "/<b..>?<c>&<dog>" => -2,       // wild path, wild query
///
///     "/<b>/<c>" => -1,               // wild path, no query
///     "/<a>/<b..>" => -1,             // wild path, no query
///     "/<b..>" => -1,                 // wild path, no query
/// }
/// ```
#[derive(Clone)]
pub struct Route {
    /// The name of this route, if one was given.
    pub name: Option<Cow<'static, str>>,
    /// The method this route matches, or `None` to match any method.
    pub method: Option<Method>,
    /// The function that should be called when the route matches.
    pub handler: Box<dyn Handler>,
    /// The route URI.
    pub uri: RouteUri<'static>,
    /// The rank of this route. Lower ranks have higher priorities.
    pub rank: isize,
    /// The media type this route matches against, if any.
    pub format: Option<MediaType>,
    /// The discovered sentinels.
    pub(crate) sentinels: Vec<Sentry>,
    /// The file, line, and column where the route was defined, if known.
    pub(crate) location: Option<(&'static str, u32, u32)>,
}

impl Route {
    /// Creates a new route with the given method, path, and handler with a base
    /// of `/` and a computed [default rank](#default-ranking).
    ///
    /// # Panics
    ///
    /// Panics if `path` is not a valid Rocket route URI.
    ///
    /// A valid route URI is any valid [`Origin`](uri::Origin) URI that is
    /// normalized, that is, does not contain any empty segments except for an
    /// optional trailing slash. Unlike a strict `Origin`, route URIs are also
    /// allowed to contain any UTF-8 characters.
    ///
    /// # Example
    ///
    /// ```rust
    /// use rocket::Route;
    /// use rocket::http::Method;
    /// # use rocket::route::dummy_handler as handler;
    ///
    /// // this is a route matching requests to `GET /`
    /// let index = Route::new(Method::Get, "/", handler);
    /// assert_eq!(index.rank, -9);
    /// assert_eq!(index.method, Some(Method::Get));
    /// assert_eq!(index.uri, "/");
    /// ```
    #[track_caller]
    pub fn new<M: Into<Option<Method>>, H: Handler>(method: M, uri: &str, handler: H) -> Route {
        Route::ranked(None, method.into(), uri, handler)
    }

    /// Creates a new route with the given rank, method, path, and handler with
    /// a base of `/`. If `rank` is `None`, the computed [default
    /// rank](#default-ranking) is used.
    ///
    /// # Panics
    ///
    /// Panics if `path` is not a valid Rocket route URI.
    ///
    /// A valid route URI is any valid [`Origin`](uri::Origin) URI that is
    /// normalized, that is, does not contain any empty segments except for an
    /// optional trailing slash. Unlike a strict `Origin`, route URIs are also
    /// allowed to contain any UTF-8 characters.
    ///
    /// # Example
    ///
    /// ```rust
    /// use rocket::Route;
    /// use rocket::http::Method;
    /// # use rocket::route::dummy_handler as handler;
    ///
    /// let foo = Route::ranked(1, Method::Post, "/foo?bar", handler);
    /// assert_eq!(foo.rank, 1);
    /// assert_eq!(foo.method, Some(Method::Post));
    /// assert_eq!(foo.uri, "/foo?bar");
    ///
    /// let foo = Route::ranked(None, Method::Post, "/foo?bar", handler);
    /// assert_eq!(foo.rank, -12);
    /// assert_eq!(foo.method, Some(Method::Post));
    /// assert_eq!(foo.uri, "/foo?bar");
    /// ```
    #[track_caller]
    pub fn ranked<M, H, R>(rank: R, method: M, uri: &str, handler: H) -> Route
        where M: Into<Option<Method>>,
              H: Handler + 'static,
              R: Into<Option<isize>>,
    {
        let uri = RouteUri::new("/", uri);
        let rank = rank.into().unwrap_or_else(|| uri.default_rank());
        Route {
            name: None,
            format: None,
            sentinels: Vec::new(),
            handler: Box::new(handler),
            location: None,
            method: method.into(),
            rank,
            uri,
        }
    }

    /// Prefix `base` to any existing mount point base in `self`.
    ///
    /// If the the current mount point base is `/`, then the base is replaced by
    /// `base`. Otherwise, `base` is prefixed to the existing `base`.
    ///
    /// ```rust
    /// use rocket::Route;
    /// use rocket::http::Method;
    /// # use rocket::route::dummy_handler as handler;
    /// # use rocket::uri;
    ///
    /// // The default base is `/`.
    /// let index = Route::new(Method::Get, "/foo/bar", handler);
    ///
    /// // Since the base is `/`, rebasing replaces the base.
    /// let rebased = index.rebase(uri!("/boo"));
    /// assert_eq!(rebased.uri.base(), "/boo");
    ///
    /// // Now every rebase prefixes.
    /// let rebased = rebased.rebase(uri!("/base"));
    /// assert_eq!(rebased.uri.base(), "/base/boo");
    ///
    /// // Rebasing to `/` does nothing.
    /// let rebased = rebased.rebase(uri!("/"));
    /// assert_eq!(rebased.uri.base(), "/base/boo");
    ///
    /// // Note that trailing slashes are preserved:
    /// let index = Route::new(Method::Get, "/foo", handler);
    /// let rebased = index.rebase(uri!("/boo/"));
    /// assert_eq!(rebased.uri.base(), "/boo/");
    /// ```
    pub fn rebase(mut self, base: uri::Origin<'_>) -> Self {
        let new_base = match self.uri.base().as_str() {
            "/" => base.path().to_string(),
            _ => format!("{}{}", base.path(), self.uri.base()),
        };

        self.uri = RouteUri::new(&new_base, &self.uri.unmounted_origin.to_string());
        self
    }

    /// Maps the `base` of this route using `mapper`, returning a new `Route`
    /// with the returned base.
    ///
    /// **Note:** Prefer to use [`Route::rebase()`] whenever possible!
    ///
    /// `mapper` is called with the current base. The returned `String` is used
    /// as the new base if it is a valid URI. If the returned base URI contains
    /// a query, it is ignored. Returns an error if the base produced by
    /// `mapper` is not a valid origin URI.
    ///
    /// # Example
    ///
    /// ```rust
    /// use rocket::Route;
    /// use rocket::http::Method;
    /// # use rocket::route::dummy_handler as handler;
    /// # use rocket::uri;
    ///
    /// let index = Route::new(Method::Get, "/foo/bar", handler);
    /// assert_eq!(index.uri.base(), "/");
    /// assert_eq!(index.uri.unmounted().path(), "/foo/bar");
    /// assert_eq!(index.uri.path(), "/foo/bar");
    ///
    /// # let old_index = index;
    /// # let index = old_index.clone();
    /// let mapped = index.map_base(|base| format!("{}{}", "/boo", base)).unwrap();
    /// assert_eq!(mapped.uri.base(), "/boo/");
    /// assert_eq!(mapped.uri.unmounted().path(), "/foo/bar");
    /// assert_eq!(mapped.uri.path(), "/boo/foo/bar");
    ///
    /// // Note that this produces different `base` results than `rebase`!
    /// # let index = old_index.clone();
    /// let rebased = index.rebase(uri!("/boo"));
    /// assert_eq!(rebased.uri.base(), "/boo");
    /// assert_eq!(rebased.uri.unmounted().path(), "/foo/bar");
    /// assert_eq!(rebased.uri.path(), "/boo/foo/bar");
    /// ```
    pub fn map_base<'a, F>(mut self, mapper: F) -> Result<Self, uri::Error<'static>>
        where F: FnOnce(uri::Origin<'a>) -> String
    {
        let base = mapper(self.uri.base);
        self.uri = RouteUri::try_new(&base, &self.uri.unmounted_origin.to_string())?;
        Ok(self)
    }
}

impl fmt::Debug for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Route")
            .field("name", &self.name)
            .field("method", &self.method)
            .field("uri", &self.uri)
            .field("rank", &self.rank)
            .field("format", &self.format)
            .finish()
    }
}

/// Information generated by the `route` attribute during codegen.
#[doc(hidden)]
pub struct StaticInfo {
    /// The route's name, i.e, the name of the function.
    pub name: &'static str,
    /// The route's method.
    pub method: Option<Method>,
    /// The route's URi, without the base mount point.
    pub uri: &'static str,
    /// The route's format, if any.
    pub format: Option<MediaType>,
    /// The route's handler, i.e, the annotated function.
    pub handler: for<'r> fn(&'r crate::Request<'_>, crate::Data<'r>) -> BoxFuture<'r>,
    /// The route's rank, if any.
    pub rank: Option<isize>,
    /// Route-derived sentinels, if any.
    /// This isn't `&'static [SentryInfo]` because `type_name()` isn't `const`.
    pub sentinels: Vec<Sentry>,
    /// The file, line, and column where the route was defined.
    pub location: (&'static str, u32, u32),
}

#[doc(hidden)]
impl From<StaticInfo> for Route {
    fn from(info: StaticInfo) -> Route {
        // This should never panic since `info.path` is statically checked.
        let uri = RouteUri::new("/", info.uri);

        Route {
            name: Some(info.name.into()),
            method: info.method,
            handler: Box::new(info.handler),
            rank: info.rank.unwrap_or_else(|| uri.default_rank()),
            format: info.format,
            sentinels: info.sentinels.into_iter().collect(),
            location: Some(info.location),
            uri,
        }
    }
}
