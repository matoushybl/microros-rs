# microROS in Rust

This project contains **very** experimental bindings for microROS based on the examples for the Raspberry Pi Pico.

## Problems
* currently works only on Cortex-M0+
* the code is riddled with unsafe without much thought about lifetimes etc.
* the API is not very friendly to use
* microROS is written in a blocking manner, meaning that the microROS transport must run with higher priority than the node/services/publishers/subscribers
* only USB transport is supported and is not implemented in a fail-safe way, given that it needs to exchange data between a completely blocking and async driven contex
* only a handful of messages have been added to the bindings
* the library allocates and the allocated memory leaks (no RAII was implemented yet)

## Examples

* `eir/src/bin/publisher.rs` - Creates a publisher that publishes `std_msgs/Int32` whose `data` field increments every time. The topic is `/pico_publisher`
* `eir/src/bin/subscriber.rs` - Creates a subscriber that subscribes to `std_msgs/Int32` on topic `/pico_subscriber`
* `eir/src/bin/service_server.rs` - Creates a service server that responds to `std_srvs/SetBool` service requests. The service's name is `/pico_srv`.
* `eir/src/bin/service_client.rs` - Creates a service client that calls a `/hello_service` service. The service's type is `std_srvs/SetBool`.
* `eir/src/bin/eir.rs` - A more complicated example used for a robot manager board.

## License

The microROS pico examples are licensed under Apache License 2.0. 
Since I am not sure how should this derivative work be licensed, license has been ommited for now, but keep the original license in mindwhen using this software.
