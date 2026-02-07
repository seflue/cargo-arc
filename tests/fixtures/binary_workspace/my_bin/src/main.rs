fn main() {
    #[cfg(feature = "a")]
    {
        use my_lib::config::Config;
        use my_lib::engine::Engine;
        let _engine = Engine;
        let _config = Config { verbose: true };
        my_lib::init();

        // Path expressions without use-import (ca-0156: path-ref dependencies)
        my_lib::engine::Engine::run(&_engine);
        let _cfg2: my_lib::config::Config = Config { verbose: false };
    }
}
